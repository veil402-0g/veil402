//! Composition root: builds the daemon from its configuration and runs it.

use std::{
    env, future,
    num::NonZeroU64,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::{
    hex,
    primitives::Address,
    providers::{Provider, ProviderBuilder},
};
use thiserror::Error;
use tokio::time::sleep;
use veil402_sdk::{Config as Proving, Owner, Secret, Veil, evm::PoolClient};

use crate::{
    adapters::{self, ChainSource, LocalProver, RemoteEnclave, SqliteStore, Window},
    core::{
        StoreError,
        envelope::{Broadcaster, ChainError, EnclaveError, EnvelopeError, EnvelopeService, Terms},
        index::{IndexError, IndexService},
        wallet::{WalletError, WalletService},
    },
};

const DATABASE: &str = "sqlite://veil402.db?mode=rwc";

/// Where the transaction engine's own durable state lives. Separate from the
/// index: it is the engine's, and it rebroadcasts from it on its own.
const WALLET_STATE: &str = "veil402-wallet.redb";

/// Blocks per log query. Relays cap the span; a thousand is under every cap
/// we have met.
const CHUNK: NonZeroU64 = NonZeroU64::new(1_000).unwrap();

/// How the sync loop reacts to failure.
#[derive(Clone, Copy, Debug)]
pub struct Retry {
    /// Delay between passes, doubled for each consecutive failure.
    pub interval: Duration,
    /// Consecutive failures tolerated before the loop gives up.
    pub attempts: u32,
}

/// Seconds since the epoch, which is the clock holds and staleness use.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Everything the daemon needs to run.
#[derive(Debug)]
pub struct Config {
    /// EVM endpoint the pool is read through.
    pub rpc: String,
    /// Deployed pool, and the two addresses a proof is bound to.
    pub pool: Address,
    pub token: Address,
    pub verifier: Address,
    /// `SQLite` URL holding the index and the notes we own.
    pub database: String,
    /// Key that pays for the daemon's transactions. Gas only: it signs for no
    /// note and holds no balance the pool recognises.
    pub signer: String,
    /// Where the transaction engine keeps the calls it is still tracking.
    pub wallet_state: PathBuf,
    /// The gnark worker, and the proving artifacts it is pinned to.
    pub worker: PathBuf,
    pub artifacts: PathBuf,
    /// Where the enclave is served from. The daemon never holds its keys.
    pub gateway: String,
    /// Who this daemon buys from, and what one envelope reserves from them.
    pub seller: Address,
    pub ceiling: u64,
    /// How long one envelope holds its reservation for.
    pub lock_for: u64,
    /// Keys the notes belong to. Losing them loses the balance.
    pub owner: Owner,
    /// Size a settlement should leave behind as a second note. Zero returns
    /// change as one note and lets the wallet fragment.
    pub target: u64,
    /// How far one pass reads.
    pub window: Window,
    /// How failure is handled.
    pub retry: Retry,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("{0} does not hold a valid value")]
    Invalid(&'static str),
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn key(name: &'static str) -> Result<Secret, ConfigError> {
    let bytes: [u8; 32] = hex::decode(required(name)?)
        .map_err(|_| ConfigError::Invalid(name))?
        .try_into()
        .map_err(|_| ConfigError::Invalid(name))?;
    Secret::from_bytes(bytes).map_err(|_| ConfigError::Invalid(name))
}

fn address(name: &'static str) -> Result<Address, ConfigError> {
    required(name)?.parse().map_err(|_| ConfigError::Invalid(name))
}

fn parsed<T: FromStr>(name: &'static str, fallback: T) -> Result<T, ConfigError> {
    match env::var(name) {
        Err(_) => Ok(fallback),
        Ok(value) => value.parse().map_err(|_| ConfigError::Invalid(name)),
    }
}

impl Config {
    /// Reads the configuration from the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a required variable is absent or a value
    /// cannot be parsed.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            rpc: required("VEIL_RPC_URL")?,
            pool: address("VEIL_POOL")?,
            token: address("VEIL_TOKEN")?,
            verifier: address("VEIL_VERIFIER")?,
            database: env::var("VEIL_DATABASE").unwrap_or_else(|_| DATABASE.to_owned()),
            signer: required("VEIL_SIGNER_KEY")?,
            wallet_state: parsed("VEIL_WALLET_STATE", PathBuf::from(WALLET_STATE))?,
            worker: PathBuf::from(required("VEIL_WORKER")?),
            artifacts: PathBuf::from(required("VEIL_ARTIFACTS")?),
            gateway: required("VEIL_GATEWAY_URL")?,
            seller: address("VEIL_SELLER")?,
            ceiling: parsed("VEIL_ENVELOPE_CEILING", 1_000)?,
            lock_for: parsed("VEIL_ENVELOPE_WINDOW", 600)?,
            owner: Owner {
                spend: key("VEIL_SPEND_KEY")?,
                view: key("VEIL_VIEW_KEY")?,
            },
            target: parsed("VEIL_NOTE_TARGET", 0)?,
            window: Window {
                chunk: parsed("VEIL_CHUNK", CHUNK)?,
                confirmations: parsed("VEIL_CONFIRMATIONS", 0)?,
            },
            retry: Retry {
                interval: Duration::from_secs(parsed("VEIL_INTERVAL", 5)?),
                attempts: parsed("VEIL_ATTEMPTS", 8)?,
            },
        })
    }
}

#[derive(Debug, Error)]
pub enum StartError {
    #[error(transparent)]
    Broadcast(#[from] ChainError),
    #[error(transparent)]
    Enclave(#[from] EnclaveError),
    #[error("the pool at {0} could not be read")]
    Pool(Address),
    #[error("the proving artifacts could not be opened")]
    Artifacts,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("the index store could not be opened")]
    Store(#[from] StoreError),
    #[error("the chain endpoint at {0} could not be reached")]
    Chain(String),
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Why one pass of the loop failed.
#[derive(Debug, Error)]
pub enum PassError {
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Wallet(#[from] WalletError),
    #[error(transparent)]
    Chain(#[from] ChainError),
}

impl PassError {
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        match self {
            Self::Envelope(error) => error.recoverable(),
            Self::Index(error) => error.recoverable(),
            Self::Wallet(error) => error.recoverable(),
            Self::Chain(error) => error.recoverable(),
        }
    }
}

/// Why the sync loop stopped. It only stops for what reading again cannot fix.
#[derive(Debug, Error)]
pub enum Halted {
    /// Something is wrong rather than behind: an index that diverged hands out
    /// paths that are wrong with it, and a corrupt store is no better.
    #[error("the daemon can no longer be trusted: {0}")]
    Untrusted(PassError),
    #[error("the daemon failed {attempts} passes in a row: {source}")]
    Exhausted { attempts: u32, source: PassError },
}

/// The chain index, the notes we own, and the loop that keeps both current.
pub struct Daemon {
    index: Arc<IndexService>,
    wallet: Arc<WalletService>,
    envelopes: Option<Arc<EnvelopeService>>,
    broadcast: Arc<dyn Broadcaster>,
    retry: Retry,
}

impl Daemon {
    /// Opens the store, connects to the chain and restores the index.
    ///
    /// # Errors
    ///
    /// Returns [`StartError`] when any of the three cannot be reached.
    pub async fn start(config: Config) -> Result<Self, StartError> {
        let store = Arc::new(SqliteStore::open(&config.database).await?);
        let broadcast = Arc::new(
            adapters::broadcaster(
                &config.rpc,
                &config.signer,
                config.pool,
                &config.wallet_state,
                store.clone(),
                config.window.confirmations,
            )
            .await?,
        );
        let provider = ProviderBuilder::new()
            .connect(&config.rpc)
            .await
            .map_err(|_| StartError::Chain(config.rpc.clone()))?;
        let chain_id = provider
            .get_chain_id()
            .await
            .map_err(|_| StartError::Chain(config.rpc.clone()))?;
        let source = Arc::new(ChainSource::new(provider, config.pool, config.window));
        let index = Arc::new(IndexService::load(source, store.clone()).await?);

        let pool = PoolClient::new(config.pool, config.token, config.verifier, chain_id)
            .map_err(|_| StartError::Pool(config.pool))?;
        let change_owner = config
            .owner
            .public()
            .map_err(|_| StartError::Pool(config.pool))?;
        let veil = Arc::new(
            Veil::open(Proving::new(config.worker, config.artifacts))
                .map_err(|_| StartError::Artifacts)?,
        );
        let enclave = Arc::new(RemoteEnclave::new(&config.gateway, pool)?);

        let wallet = Arc::new(WalletService::new(store.clone(), store.clone(), config.owner));
        let envelopes = Arc::new(EnvelopeService::new(
            wallet.clone(),
            index.clone(),
            enclave,
            broadcast.clone(),
            Arc::new(LocalProver::new(veil, pool)),
            store,
            Terms {
                pool,
                seller: config.seller,
                ceiling: config.ceiling,
                window: config.lock_for,
                change_owner,
                target: config.target,
            },
        ));
        Ok(Self::new(index, wallet, broadcast, config.retry).serving(envelopes))
    }

    /// Builds a daemon over services that are already wired.
    #[must_use]
    pub const fn new(
        index: Arc<IndexService>,
        wallet: Arc<WalletService>,
        broadcast: Arc<dyn Broadcaster>,
        retry: Retry,
    ) -> Self {
        Self {
            index,
            wallet,
            envelopes: None,
            broadcast,
            retry,
        }
    }

    /// Gives the daemon the envelopes it serves sessions from.
    #[must_use]
    pub fn serving(mut self, envelopes: Arc<EnvelopeService>) -> Self {
        self.envelopes = Some(envelopes);
        self
    }

    /// The notes this daemon owns.
    #[must_use]
    pub const fn wallet(&self) -> &Arc<WalletService> {
        &self.wallet
    }

    /// The envelopes, for the API that opens them.
    #[must_use]
    pub const fn envelopes(&self) -> Option<&Arc<EnvelopeService>> {
        self.envelopes.as_ref()
    }

    /// The index, for anything that reads it.
    #[must_use]
    pub const fn index(&self) -> &Arc<IndexService> {
        &self.index
    }

    /// Syncs until the process is signalled, or until the index can no longer
    /// be trusted.
    ///
    /// Being behind is not a failure, so a pass that only lags keeps the loop
    /// going. A divergence stops it at once: an index that is wrong hands out
    /// paths that cost a proof and the gas to have it rejected.
    ///
    /// # Errors
    ///
    /// Returns [`Halted`] when the loop stops for a reason reading again
    /// cannot fix.
    pub async fn run(&self) -> Result<(), Halted> {
        tokio::select! {
            () = signalled() => Ok(()),
            halted = self.syncing() => Err(halted),
        }
    }

    /// One pass: advance what is in flight, catch the index up, then catch
    /// the notes up with it.
    ///
    /// Ticking first is deliberate — a lock that confirms in this pass should
    /// be visible to the index read that follows, not a pass later.
    async fn pass(&self) -> Result<(), PassError> {
        self.broadcast.tick().await?;
        let health = self.index.sync().await?;
        let seen = self.wallet.reconcile(now()).await?;
        tracing::debug!(
            leaves = health.leaves,
            block = health.block,
            placed = seen.placed,
            spent = seen.spent,
            escrowed = seen.escrowed,
            "synced"
        );
        if seen.stale > 0 {
            tracing::warn!(notes = seen.stale, "written down but never placed");
        }
        Ok(())
    }

    async fn syncing(&self) -> Halted {
        let mut failures = 0;
        loop {
            match self.pass().await {
                Ok(()) => failures = 0,
                Err(error) if !error.recoverable() => return Halted::Untrusted(error),
                Err(source) => {
                    failures += 1;
                    if failures >= self.retry.attempts {
                        return Halted::Exhausted {
                            attempts: failures,
                            source,
                        };
                    }
                    tracing::warn!(error = %source, failures, "pass failed");
                }
            }
            sleep(self.backoff(failures)).await;
        }
    }

    /// Doubles the delay for each consecutive failure, so an endpoint that is
    /// briefly away is waited out rather than hammered.
    fn backoff(&self, failures: u32) -> Duration {
        self.retry.interval * 2_u32.pow(failures.min(5))
    }
}

async fn signalled() {
    if tokio::signal::ctrl_c().await.is_err() {
        future::pending::<()>().await;
    }
}
