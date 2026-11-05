//! Composition root: builds the daemon from its configuration and runs it.

use std::{
    env, future,
    num::NonZeroU64,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::{hex, primitives::Address, providers::ProviderBuilder};
use thiserror::Error;
use tokio::time::sleep;
use veil402_sdk::{Owner, Secret};

use crate::{
    adapters::{ChainSource, SqliteStore, Window},
    core::{
        StoreError,
        index::{IndexError, IndexService},
        wallet::{WalletError, WalletService},
    },
};

const DATABASE: &str = "sqlite://veil402.db?mode=rwc";

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
    /// Deployed pool.
    pub pool: Address,
    /// `SQLite` URL holding the index and the notes we own.
    pub database: String,
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
            pool: required("VEIL_POOL")?
                .parse()
                .map_err(|_| ConfigError::Invalid("VEIL_POOL"))?,
            database: env::var("VEIL_DATABASE").unwrap_or_else(|_| DATABASE.to_owned()),
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
    Index(#[from] IndexError),
    #[error(transparent)]
    Wallet(#[from] WalletError),
}

impl PassError {
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        match self {
            Self::Index(error) => error.recoverable(),
            Self::Wallet(error) => error.recoverable(),
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
        let provider = ProviderBuilder::new()
            .connect(&config.rpc)
            .await
            .map_err(|_| StartError::Chain(config.rpc.clone()))?;
        let source = Arc::new(ChainSource::new(provider, config.pool, config.window));
        let index = Arc::new(IndexService::load(source, store.clone()).await?);
        let wallet = WalletService::new(store.clone(), store, config.owner);
        Ok(Self::new(index, Arc::new(wallet), config.retry))
    }

    /// Builds a daemon over services that are already wired.
    #[must_use]
    pub const fn new(
        index: Arc<IndexService>,
        wallet: Arc<WalletService>,
        retry: Retry,
    ) -> Self {
        Self {
            index,
            wallet,
            retry,
        }
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

    /// One pass: catch the index up, then catch the notes up with it.
    async fn pass(&self) -> Result<(), PassError> {
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
