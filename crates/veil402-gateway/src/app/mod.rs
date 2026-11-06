//! Composition root: builds the gateway from its configuration and serves it.

use std::{env, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use alloy::{hex, primitives::Address};
use thiserror::Error;
use tower_http::trace::TraceLayer;
use veil402_enclave::{Upstream, adapters::Router};
use veil402_sdk::{
    Config as Proving, Veil,
    evm::PoolClient,
    session::{Keyring, RootSeed},
};

use crate::{
    adapters::{self, Enclave, Replays, Sessions, http::router},
    core::{Escrows, MeterError, MeteringService, Pricing},
};

/// Where the gateway listens when nothing says otherwise.
const BIND: &str = "0.0.0.0:8402";

/// Where the sealed session records live when nothing says otherwise.
const STATE: &str = "veil402-sessions.redb";

/// Only providers that seal the request and keep nothing after it.
const TRUST: &str = "private";

/// Micro-USD a single call may spend when it names no limit of its own.
/// Reasoning models will take everything they are given, so this is a real
/// bound rather than a formality.
const CEILING: u32 = 4_096;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("{0} does not hold a valid value")]
    Invalid(&'static str),
}

/// Everything the gateway needs to serve.
#[derive(Debug)]
pub struct Config {
    pub pool: PoolClient,
    /// Seed every enclave key derives from. On real hardware this comes from
    /// KMS gated on attestation; here it is configuration.
    ///
    /// Raw bytes, not a field element: it keys an HKDF, not a circuit.
    pub root: [u8; 32],
    pub worker: PathBuf,
    pub artifacts: PathBuf,
    /// Who the pool pays. One seller, so it is settled at boot.
    pub seller: Address,
    /// Endpoint the pool is read through. Read-only: the gateway holds no
    /// key that moves anything.
    pub rpc: String,
    /// Where the host keeps what a restarted enclave needs. Opaque: every
    /// record in it is sealed by the enclave to itself.
    pub state: PathBuf,
    pub bind: SocketAddr,
    /// Where inference is bought, and with whose credit. On real hardware
    /// the key is sealed to the enclave; here it is configuration.
    pub upstream: String,
    pub upstream_key: String,
    /// Which providers are acceptable. `private` is the only one that both
    /// runs the weights in an enclave and refuses to fall back to one that
    /// does not.
    pub trust: String,
    /// Pool units per micro-USD, and what we add over cost. Separate
    /// because the first is a fact about the token and the second is a
    /// decision.
    pub scale: u64,
    pub margin_bps: u64,
    /// What one call may spend when the request names no limit of its own.
    pub ceiling: u32,
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn parsed<T: FromStr>(name: &'static str, fallback: T) -> Result<T, ConfigError> {
    match env::var(name) {
        Err(_) => Ok(fallback),
        Ok(value) => value.parse().map_err(|_| ConfigError::Invalid(name)),
    }
}

fn address(name: &'static str) -> Result<Address, ConfigError> {
    required(name)?
        .parse()
        .map_err(|_| ConfigError::Invalid(name))
}

impl Config {
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a required variable is absent or a value
    /// cannot be parsed.
    pub fn from_env() -> Result<Self, ConfigError> {
        let pool = PoolClient::new(
            address("VEIL_POOL")?,
            address("VEIL_TOKEN")?,
            address("VEIL_VERIFIER")?,
            parsed("VEIL_CHAIN_ID", 296)?,
        )
        .map_err(|_| ConfigError::Invalid("VEIL_POOL"))?;
        Ok(Self {
            pool,
            root: hex::decode(required("VEIL_ROOT_SEED")?)
                .map_err(|_| ConfigError::Invalid("VEIL_ROOT_SEED"))?
                .try_into()
                .map_err(|_| ConfigError::Invalid("VEIL_ROOT_SEED"))?,
            rpc: required("VEIL_RPC_URL")?,
            worker: PathBuf::from(required("VEIL_WORKER")?),
            artifacts: PathBuf::from(required("VEIL_ARTIFACTS")?),
            seller: address("VEIL_SELLER")?,
            state: PathBuf::from(
                env::var("VEIL_GATEWAY_STATE").unwrap_or_else(|_| STATE.to_owned()),
            ),
            bind: parsed(
                "VEIL_GATEWAY_BIND",
                BIND.parse().map_err(|_| ConfigError::Invalid("bind"))?,
            )?,
            upstream: required("VEIL_0G_BASE")?,
            upstream_key: required("VEIL_0G_KEY")?,
            trust: env::var("VEIL_0G_TRUST_MODE").unwrap_or_else(|_| TRUST.to_owned()),
            scale: parsed("VEIL_TOKEN_SCALE", 1)?,
            margin_bps: parsed("VEIL_MARGIN_BPS", 0)?,
            ceiling: parsed("VEIL_CALL_CEILING", CEILING)?,
        })
    }
}

#[derive(Debug, Error)]
pub enum StartError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Meter(#[from] MeterError),
    #[error("the proving artifacts could not be opened")]
    Artifacts,
    #[error(transparent)]
    Upstream(#[from] veil402_enclave::UpstreamError),
    #[error("{0} could not be listened on")]
    Bind(SocketAddr),
}

/// Builds the metering service over its adapters.
///
/// # Errors
///
/// Returns [`StartError`] when the artifacts or the root seed are unusable.
pub fn assemble(
    config: &Config,
    escrows: Arc<dyn Escrows>,
    upstream: Arc<dyn Upstream>,
) -> Result<Arc<MeteringService>, StartError> {
    let veil = Arc::new(
        Veil::open(Proving::new(
            config.worker.clone(),
            config.artifacts.clone(),
        ))
        .map_err(|_| StartError::Artifacts)?,
    );
    let enclave = Enclave::new(
        config.pool,
        Keyring::new(RootSeed::from_bytes(config.root)),
        veil,
        upstream,
        Pricing::new(config.scale, config.margin_bps),
    )?;
    tracing::info!(signer = %enclave.signer(), "the registry must vouch for this address");
    Ok(Arc::new(MeteringService::new(
        Arc::new(enclave),
        Arc::new(Replays::default()),
        Arc::new(Sessions::open(&config.state)?),
        escrows,
        config.pool,
        config.seller,
        Vec::new(),
    )))
}

/// Serves until the process is signalled.
///
/// # Errors
///
/// Returns [`StartError::Bind`] when the address is unavailable.
pub async fn serve(config: Config) -> Result<(), StartError> {
    let escrows = Arc::new(adapters::connect(&config.rpc, config.pool.address()).await?);
    let upstream = Arc::new(Router::new(
        &config.upstream,
        &config.upstream_key,
        &config.trust,
        config.ceiling,
    )?);
    let service = assemble(&config, escrows, upstream)?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| StartError::Bind(config.bind))?;
    tracing::info!(bind = %config.bind, "serving");
    axum::serve(
        listener,
        router(service).layer(TraceLayer::new_for_http()),
    )
    // A deploy should drain sessions rather than drop them.
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|_| StartError::Bind(config.bind))
}
