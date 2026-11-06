//! Composition root: builds the gateway from its configuration and serves it.

use std::{env, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use alloy::{hex, primitives::Address};
use thiserror::Error;
use tower_http::trace::TraceLayer;
use veil402_sdk::{
    Config as Proving, Veil,
    evm::PoolClient,
    session::{Keyring, RootSeed},
    wire::Offer,
};

use crate::{
    adapters::{Catalogue, Enclave, Replays, Sessions, http::router},
    core::{MeterError, MeteringService},
};

/// Where the gateway listens when nothing says otherwise.
const BIND: &str = "0.0.0.0:8402";

/// Where the sealed session records live when nothing says otherwise.
const STATE: &str = "veil402-sessions.redb";

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
    pub offers: Vec<Offer>,
    /// Where the host keeps what a restarted enclave needs. Opaque: every
    /// record in it is sealed by the enclave to itself.
    pub state: PathBuf,
    pub bind: SocketAddr,
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
            worker: PathBuf::from(required("VEIL_WORKER")?),
            artifacts: PathBuf::from(required("VEIL_ARTIFACTS")?),
            offers: vec![Offer {
                model: required("VEIL_MODEL")?,
                seller: address("VEIL_SELLER")?,
                rate: parsed("VEIL_RATE", 1)?,
            }],
            state: PathBuf::from(
                env::var("VEIL_GATEWAY_STATE").unwrap_or_else(|_| STATE.to_owned()),
            ),
            bind: parsed(
                "VEIL_GATEWAY_BIND",
                BIND.parse().map_err(|_| ConfigError::Invalid("bind"))?,
            )?,
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
    #[error("{0} could not be listened on")]
    Bind(SocketAddr),
}

/// Builds the metering service over its adapters.
///
/// # Errors
///
/// Returns [`StartError`] when the artifacts or the root seed are unusable.
pub fn assemble(config: &Config) -> Result<Arc<MeteringService>, StartError> {
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
    )?;
    tracing::info!(signer = %enclave.signer(), "the registry must vouch for this address");
    Ok(Arc::new(MeteringService::new(
        Arc::new(enclave),
        Arc::new(Catalogue::new(config.offers.clone())),
        Arc::new(Replays::default()),
        Arc::new(Sessions::open(&config.state)?),
        config.pool,
        Vec::new(),
    )))
}

/// Serves until the process is signalled.
///
/// # Errors
///
/// Returns [`StartError::Bind`] when the address is unavailable.
pub async fn serve(config: Config) -> Result<(), StartError> {
    let service = assemble(&config)?;
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
