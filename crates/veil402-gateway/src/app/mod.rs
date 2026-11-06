//! Composition root: builds the gateway from its configuration and serves it.

use std::{
    collections::HashMap, env, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc,
    time::Duration,
};

use alloy::{hex, primitives::Address};
use thiserror::Error;
use tokio::time::sleep;
use tower_http::trace::TraceLayer;
use veil402_enclave::{Attestor, Upstream, adapters::Router};
use veil402_sdk::{
    Config as Proving, Veil,
    evm::PoolClient,
    session::{Keyring, RootSeed},
};

use crate::{
    adapters::{self, Enclave, Replays, Sessions, http::router},
    core::{Escrows, MeterError, MeteringService, Pricing, Settles},
};

/// Where the gateway listens when nothing says otherwise.
const BIND: &str = "0.0.0.0:8402";

/// Where the sealed session records live when nothing says otherwise.
const STATE: &str = "veil402-sessions.redb";

/// Only providers that seal the request and keep nothing after it.
const TRUST: &str = "private";

/// Multicall3, at the address it holds on every chain that has it.
const MULTICALL: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

/// Where the sending engine keeps what it is still tracking.
const WALLET_STATE: &str = "veil402-gateway-wallet.redb";

/// How often to look for sessions the buyer has stopped settling.
const SWEEP: Duration = Duration::from_secs(30);

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
    /// Most this gateway will pay per unit of gas, in wei.
    pub fee_cap: u128,
    /// Providers whose signing key an attested quote has been checked
    /// against, out of band, and where each answers for what it signed.
    pub vouched: HashMap<Address, Attestor>,
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
    /// Pays the gas on a settlement the buyer never asked for. Gas only: a
    /// settlement is authorised by the enclave's signature and the proof,
    /// and the pool does not look at who sent it.
    pub signer: String,
    pub multicall: Address,
    pub wallet_state: PathBuf,
    /// Blocks a settlement waits for. Hedera reaches finality without
    /// reorgs, so zero is correct there.
    pub confirmations: u64,
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
            fee_cap: parsed("VEIL_MAX_FEE_GWEI", MAX_FEE_GWEI)? * GWEI,
            vouched: vouched(&env::var("VEIL_TEE_PROVIDERS").unwrap_or_default())?,
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
            signer: required("VEIL_GATEWAY_SIGNER")?,
            multicall: parsed(
                "VEIL_MULTICALL",
                MULTICALL
                    .parse()
                    .map_err(|_| ConfigError::Invalid("VEIL_MULTICALL"))?,
            )?,
            wallet_state: parsed("VEIL_GATEWAY_WALLET_STATE", PathBuf::from(WALLET_STATE))?,
            confirmations: parsed("VEIL_CONFIRMATIONS", 0)?,
        })
    }
}

/// Most either process will pay per unit of gas.
///
/// A cap rather than a price: the oracle decides what a call costs, and this
/// only stops a spike from draining the key that pays for them. The
/// library's default is an Ethereum number, and Hedera prices gas to a
/// dollar target — about a thousand gwei — so that default refuses every
/// transaction on it.
const MAX_FEE_GWEI: u128 = 20_000;

/// Gwei, in wei.
const GWEI: u128 = 1_000_000_000;

/// Reads the providers whose attestation an operator has already checked.
///
/// `<provider>|<endpoint>|<signer>`, semicolon separated. Pinned rather than
/// discovered: the quote binding a signer to real hardware is verified out
/// of band, and naming the endpoint here means a reply can never send this
/// side somewhere of its choosing.
fn vouched(configured: &str) -> Result<HashMap<Address, Attestor>, ConfigError> {
    configured
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let [provider, endpoint, signer] = *entry.split('|').collect::<Vec<_>>() else {
                return Err(ConfigError::Invalid("VEIL_TEE_PROVIDERS"));
            };
            Ok((
                provider
                    .trim()
                    .parse()
                    .map_err(|_| ConfigError::Invalid("VEIL_TEE_PROVIDERS"))?,
                Attestor {
                    endpoint: endpoint.trim().to_owned(),
                    signer: signer
                        .trim()
                        .parse()
                        .map_err(|_| ConfigError::Invalid("VEIL_TEE_PROVIDERS"))?,
                },
            ))
        })
        .collect()
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
        config.vouched.clone(),
    )?);
    let service = assemble(&config, escrows, upstream)?;
    let settler = Arc::new(
        adapters::settler(
            &config.rpc,
            &config.signer,
            &config.wallet_state,
            adapters::Sending {
                pool: config.pool.address(),
                multicall: config.multicall,
                chain_id: config.pool.chain_id(),
                confirmations: config.confirmations,
                fee_cap: config.fee_cap,
            },
        )
        .await?,
    );
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| StartError::Bind(config.bind))?;
    tracing::info!(bind = %config.bind, "serving");

    let serving = axum::serve(
        listener,
        router(service.clone()).layer(TraceLayer::new_for_http()),
    )
    // A deploy should drain sessions rather than drop them.
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    });

    tokio::select! {
        served = serving => served.map_err(|_| StartError::Bind(config.bind)),
        () = sweeping(service, settler) => Ok(()),
    }
}

/// Settles what the buyer has stopped settling.
///
/// The buyer closes a session when it wants its note back; one that has
/// stopped caring never does, and the work was delivered either way. So the
/// seller closes it instead, and a pass that finds nothing due still turns
/// the sending engine over.
async fn sweeping(service: Arc<MeteringService>, settler: Arc<dyn Settles>) {
    loop {
        sleep(SWEEP).await;
        let due = match service.closing().await {
            Ok(due) => due,
            Err(error) => {
                tracing::warn!(%error, "could not read what is due");
                continue;
            }
        };
        // Only what actually went is finished with. Anything held back is
        // still ours to carry, and comes round on the next pass.
        match settler.send(&due).await {
            Ok(sent) => {
                if !sent.is_empty() {
                    tracing::info!(settlements = sent.len(), "closed sessions the buyer left open");
                }
                if let Err(error) = service.settled(&sent).await {
                    tracing::warn!(%error, "settled sessions could not be let go of");
                }
            }
            Err(error) => tracing::warn!(%error, "settlements could not be sent"),
        }
    }
}
