//! Composition root: builds the daemon from its configuration and runs it.

use std::{env, future, str::FromStr, sync::Arc, time::Duration};

use alloy::{primitives::Address, providers::ProviderBuilder};
use thiserror::Error;
use tokio::time::sleep;

use crate::{
    adapters::{ChainSource, SqliteIndex, Window},
    core::index::{IndexError, IndexService, StoreError},
};

const DATABASE: &str = "sqlite://veil402.db?mode=rwc";

/// How the sync loop reacts to failure.
#[derive(Clone, Copy, Debug)]
pub struct Retry {
    /// Delay between passes, doubled for each consecutive failure.
    pub interval: Duration,
    /// Consecutive failures tolerated before the loop gives up.
    pub attempts: u32,
}

/// Everything the daemon needs to run.
#[derive(Clone, Debug)]
pub struct Config {
    /// EVM endpoint the pool is read through.
    pub rpc: String,
    /// Deployed pool.
    pub pool: Address,
    /// `SQLite` URL holding the index.
    pub database: String,
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
            window: Window {
                chunk: parsed("VEIL_CHUNK", 1_000)?,
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

/// Why the sync loop stopped. It only stops for what reading again cannot fix.
#[derive(Debug, Error)]
pub enum Halted {
    /// The index is wrong rather than behind, so every path it would hand out
    /// is wrong with it.
    #[error("the index can no longer be trusted: {0}")]
    Untrusted(IndexError),
    #[error("the index failed {attempts} passes in a row: {source}")]
    Exhausted { attempts: u32, source: IndexError },
}

/// The chain index and the loop that keeps it current.
pub struct Daemon {
    index: Arc<IndexService>,
    retry: Retry,
}

impl Daemon {
    /// Opens the store, connects to the chain and restores the index.
    ///
    /// # Errors
    ///
    /// Returns [`StartError`] when any of the three cannot be reached.
    pub async fn start(config: &Config) -> Result<Self, StartError> {
        let store = Arc::new(SqliteIndex::open(&config.database).await?);
        let provider = ProviderBuilder::new()
            .connect(&config.rpc)
            .await
            .map_err(|_| StartError::Chain(config.rpc.clone()))?;
        let source = Arc::new(ChainSource::new(provider, config.pool, config.window));
        Ok(Self::new(
            Arc::new(IndexService::load(source, store).await?),
            config.retry,
        ))
    }

    /// Builds a daemon over an index that is already wired.
    #[must_use]
    pub const fn new(index: Arc<IndexService>, retry: Retry) -> Self {
        Self { index, retry }
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

    async fn syncing(&self) -> Halted {
        let mut failures = 0;
        loop {
            match self.index.sync().await {
                Ok(health) => {
                    failures = 0;
                    tracing::debug!(
                        leaves = health.leaves,
                        block = health.block,
                        "index synced"
                    );
                }
                Err(error) if !error.recoverable() => return Halted::Untrusted(error),
                Err(source) => {
                    failures += 1;
                    if failures >= self.retry.attempts {
                        return Halted::Exhausted {
                            attempts: failures,
                            source,
                        };
                    }
                    tracing::warn!(error = %source, failures, "index sync failed");
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
