//! Composition root: builds the gateway from its configuration and serves it.

use std::{
    env, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc,
    time::Duration,
};

use alloy::{hex, primitives::Address};
use thiserror::Error;
use tokio::time::sleep;
use tower_http::trace::TraceLayer;
use veil402_enclave::{Upstream, adapters::Router};
use veil402_sdk::{
    Config as Proving, Veil,
    evm::PoolClient,
    session::{Keyring, RootSeed},
};

use crate::{
    adapters::{self, Enclave, Replays, Sessions, http::router},
    core::{Escrows, MeterError, MeteringService, Meters, Pricing, Settles},
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
#[derive(Clone, Debug)]
pub struct Config {
    /// Most this gateway will pay per unit of gas, in wei.
    pub fee_cap: u128,
    /// Where the serving contract that names each provider's attested key
    /// lives. Separate from the pool's chain: the two need not be the same.
    pub serving_rpc: String,
    pub serving: Address,
    pub pool: PoolClient,
    /// Seed every enclave key derives from. On real hardware this comes from
    /// KMS gated on attestation; here it is configuration.
    ///
    /// Raw bytes, not a field element: it keys an HKDF, not a circuit.
    /// The meter's root seed, when this host runs one itself.
    pub root: Option<[u8; 32]>,
    /// The enclave's context id, when the meter runs in one instead. The
    /// seed is then never in this process at all.
    pub enclave: Option<u32>,
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
            serving_rpc: env::var("VEIL_0G_CHAIN_RPC").unwrap_or_else(|_| SERVING_RPC.to_owned()),
            serving: parsed("VEIL_0G_INFERENCE", SERVING)?,
            pool,
            enclave: env::var("VEIL_ENCLAVE_CID")
                .ok()
                .map(|cid| cid.parse().map_err(|_| ConfigError::Invalid("VEIL_ENCLAVE_CID")))
                .transpose()?,
            // Absent is correct once an enclave holds it: a host that still
            // had the seed in its environment would not be a host that had
            // stopped being able to read the traffic.
            root: env::var("VEIL_ROOT_SEED")
                .ok()
                .map(|seed| {
                    hex::decode(seed)
                        .ok()
                        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
                        .ok_or(ConfigError::Invalid("VEIL_ROOT_SEED"))
                })
                .transpose()?,
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

/// 0G mainnet, where the serving contract names each provider's attested key.
const SERVING_RPC: &str = "https://evmrpc.0g.ai";
const SERVING: Address = alloy::primitives::address!("47340d900bdFec2BD393c626E12ea0656F938d84");

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
    #[error("the enclave is not answering")]
    Enclave,
    /// Neither a seed nor an enclave. Refusing is the point: a meter with no
    /// key would start, serve a 402 challenge nobody can satisfy, and look
    /// like a provider that is merely busy.
    #[error("no VEIL_ROOT_SEED and no VEIL_ENCLAVE_CID: this host would meter nothing")]
    Seedless,
}

/// Builds the metering service over its adapters.
///
/// # Errors
///
/// Returns [`StartError`] when the artifacts or the root seed are unusable.
pub async fn assemble(
    config: &Config,
    escrows: Arc<dyn Escrows>,
    upstream: Arc<dyn Upstream>,
) -> Result<Arc<MeteringService>, StartError> {
    let meters = meter(config, upstream).await?;
    tracing::info!(signer = %meters.signer(), "the registry must vouch for this address");
    Ok(Arc::new(MeteringService::new(
        meters,
        Arc::new(Replays::default()),
        Arc::new(Sessions::open(&config.state)?),
        escrows,
        config.pool,
        config.seller,
        Vec::new(),
    )))
}

/// The meter: in this process, or in an enclave beside it.
///
/// Which one is decided by whether a context id was configured, not by a
/// build flag — the same binary runs both ways, and a host that has an
/// enclave is simply a host that was never given a seed.
// Async because linking to an enclave is a round trip; on a host without
// vsock there is nothing to await, which clippy is right to notice and wrong
// to act on — the signature is the same either way or the caller forks.
#[allow(clippy::unused_async)]
async fn meter(
    config: &Config,
    upstream: Arc<dyn Upstream>,
) -> Result<Arc<dyn Meters>, StartError> {
    #[cfg(target_os = "linux")]
    if let Some(cid) = config.enclave {
        let linked = crate::adapters::link::Linked::open(crate::adapters::link::Vsock {
            cid,
            port: ENCLAVE_PORT,
        })
        .await
        .map_err(|_| StartError::Enclave)?;
        return Ok(Arc::new(linked));
    }

    let root = config.root.ok_or(StartError::Seedless)?;
    let veil = Arc::new(
        Veil::open(Proving::new(
            config.worker.clone(),
            config.artifacts.clone(),
        ))
        .map_err(|_| StartError::Artifacts)?,
    );
    Ok(Arc::new(Enclave::new(
        config.pool,
        Keyring::new(RootSeed::from_bytes(root)),
        veil,
        upstream,
        Pricing::new(config.scale, config.margin_bps),
    )?))
}

/// Where the enclave answers. Fixed rather than configured: both ends are
/// built from this repository, and a port either side could get wrong is a
/// setting that buys nothing.
pub const ENCLAVE_PORT: u32 = 5402;

/// Where the parent relays the enclave's KMS traffic.
pub const KMS_RELAY_PORT: u32 = 8000;

/// Where attested keys are looked up, or nothing if that cannot be reached.
///
/// Attestation is something a receipt carries, not something metering needs,
/// so a serving contract that is away costs the claim and not the service.
/// Every provider then answers unattested, which is the honest result.
async fn attesting(config: &Config) -> Arc<dyn veil402_enclave::Registry> {
    match adapters::registrar(&config.serving_rpc, config.serving).await {
        Ok(registrar) => Arc::new(registrar),
        Err(error) => {
            tracing::warn!(%error, "the serving contract is unreachable; nothing will be attested");
            Arc::new(std::collections::HashMap::new())
        }
    }
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
        attesting(&config).await,
    )?);
    // Before the service: the meter it links to may not exist yet, and the
    // posting is what brings it into being.
    #[cfg(target_os = "linux")]
    if let Some(cid) = config.enclave {
        let posting = config.clone();
        tokio::spawn(async move {
            adapters::posting::keep(
                adapters::link::Vsock { cid, port: ENCLAVE_PORT },
                &posting,
                env::var("VEIL_KMS_KEY").unwrap_or_default(),
                env::var("AWS_REGION").unwrap_or_else(|_| "eu-west-1".to_owned()),
                KMS_RELAY_PORT,
            )
            .await;
        });
    }
    let service = assemble(&config, escrows, upstream).await?;
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
