//! Composition root: builds the daemon from its configuration and runs it.

use std::{
    env, future,
    net::SocketAddr,
    num::NonZeroU64,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alloy::{
    hex,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    sol,
};
use thiserror::Error;
use tokio::time::sleep;
use veil402_sdk::{Config as Proving, Field, Note, Owner, Secret, Veil, evm::PoolClient};

use crate::{
    adapters::{self, Api, ChainSource, LocalProver, RemoteEnclave, SqliteStore, Window},
    core::{
        StoreError,
        agent::AgentService,
        envelope::{ChainError, EnclaveError, EnvelopeError, EnvelopeService, Terms},
        index::{IndexError, IndexService},
        wallet::{Minted, WalletError, WalletService},
    },
};

const DATABASE: &str = "sqlite://veil402.db?mode=rwc";

/// Loopback, because this process holds the keys that spend the notes and
/// speaks no TLS. Reaching it from anywhere else is a decision, not a default.
const LISTEN: &str = "127.0.0.1:8402";

/// What a call is assumed to produce when its request names no limit.
/// Only the reservation rests on it; the bill is whatever arrives.
const CALL_CAP: u32 = 8_192;

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

/// Where the transaction engine's own durable state lives. Separate from the
/// index: it is the engine's, and it rebroadcasts from it on its own.
const WALLET_STATE: &str = "veil402-wallet.redb";

/// What a session note wants to be worth when nothing says otherwise.
/// Around a dollar of inference at one pool unit per micro-USD: a few
/// thousand calls between settlements rather than a few dozen.
const SESSION: u64 = 1_000_000;

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

/// An endpoint's host, without the path or query a key usually hides in.
fn host(endpoint: &str) -> String {
    endpoint
        .split_once("//")
        .map_or(endpoint, |(_, rest)| rest)
        .split(['/', '?'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Where the index and the notes live.
fn database() -> String {
    env::var("VEIL_DATABASE").unwrap_or_else(|_| DATABASE.to_owned())
}

/// The ledger on its own, for the commands that only touch keys.
///
/// # Errors
///
/// Returns [`StartError`] when the store cannot be opened.
pub async fn agents() -> Result<AgentService, StartError> {
    Ok(AgentService::new(Arc::new(
        SqliteStore::open(&database()).await?,
    )))
}

sol! {
    #[sol(rpc)]
    interface IToken {
        function approve(address spender, uint256 value) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
    }
}

/// Shields `amount` into the pool, under this daemon's own key.
///
/// Nothing reads a note back off the chain, so the wallet is told about the
/// one this just made. Lose the keys and the balance goes with them.
///
/// # Errors
///
/// Returns [`StartError`] when the chain refuses either call, or the note
/// cannot be written down.
pub async fn deposit(amount: u64) -> Result<Field, StartError> {
    let config = Config::from_env()?;
    let provider = ProviderBuilder::new()
        .connect(&config.rpc)
        .await
        .map_err(|_| StartError::Chain(host(&config.rpc)))?;
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|_| StartError::Chain(host(&config.rpc)))?;
    let pool = PoolClient::new(config.pool, config.token, config.verifier, chain_id)
        .map_err(|_| StartError::Pool(config.pool))?;
    let sender = adapters::depositor(
        &config.rpc,
        &config.signer,
        [config.pool, config.token],
        &config.wallet_state.with_extension("deposit.redb"),
        config.fee_cap,
    )
    .await?;
    let token = IToken::new(config.token, &provider);
    let refused = |error: &dyn std::fmt::Display| StartError::Deposit(error.to_string());

    // Approved once and left standing. The pool is the only spender this key
    // ever names, and an allowance per deposit is a transaction per deposit
    // for a permission that does not change.
    let allowed = token
        .allowance(sender.account(), config.pool)
        .call()
        .await
        .map_err(|error| refused(&error))?;
    if allowed < U256::from(amount) {
        sender
            .send(
                config.token,
                token
                    .approve(config.pool, U256::MAX)
                    .calldata()
                    .to_vec(),
            )
            .await?;
    }

    let note = Note {
        value: amount,
        asset: pool.asset(),
        random: Secret::random().map_err(|_| StartError::Deposit("no randomness".into()))?,
    };
    let commitment = note
        .commitment(&config.owner)
        .map_err(|error| refused(&error))?;
    sender
        .send(
            config.pool,
            pool.shield(
                note.public_key(&config.owner).map_err(|error| refused(&error))?,
                amount,
                b"deposit",
            )
            .map_err(|error| refused(&error))?,
        )
        .await?;

    let store = Arc::new(SqliteStore::open(&config.database).await?);
    let random = Secret::from_bytes(note.random.to_bytes())
        .map_err(|_| StartError::Deposit("the note will not re-encode".into()))?;
    WalletService::new(store.clone(), store, config.owner)
        .remember(
            &[Minted {
                commitment,
                value: amount,
                asset: pool.asset(),
                random,
                seed: None,
            }],
            now(),
        )
        .await
        .map_err(|error| refused(&error))?;
    Ok(commitment)
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
    /// Who this daemon buys from.
    pub seller: Address,
    /// What a session note wants to be worth, in pool units.
    ///
    /// How much work one note buys before its envelope has to be settled and
    /// another opened, and the size a settlement tries to leave behind for
    /// the next one. A rollover costs a proof and two transactions, so it
    /// wants to be worth thousands of calls rather than tens: at one pool
    /// unit per micro-USD a real call runs sixty to two hundred and fifty.
    pub session: u64,
    /// How long one envelope holds its reservation for.
    pub lock_for: u64,
    /// Where the agent API is served from.
    pub listen: SocketAddr,
    /// Output a call is assumed to produce when it names no limit.
    pub cap: u32,
    /// Most this daemon will pay per unit of gas, in wei.
    pub fee_cap: u128,
    /// Keys the notes belong to. Losing them loses the balance.
    pub owner: Owner,
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
            database: database(),
            signer: required("VEIL_SIGNER_KEY")?,
            wallet_state: parsed("VEIL_WALLET_STATE", PathBuf::from(WALLET_STATE))?,
            worker: PathBuf::from(required("VEIL_WORKER")?),
            artifacts: PathBuf::from(required("VEIL_ARTIFACTS")?),
            gateway: required("VEIL_GATEWAY_URL")?,
            seller: address("VEIL_SELLER")?,
            session: parsed("VEIL_SESSION_NOTE", SESSION)?,
            lock_for: parsed("VEIL_ENVELOPE_WINDOW", 600)?,
            listen: parsed("VEIL_LISTEN", LISTEN.parse().map_err(|_| ConfigError::Invalid("VEIL_LISTEN"))?)?,
            cap: parsed("VEIL_CALL_CAP", CALL_CAP)?,
            fee_cap: parsed("VEIL_MAX_FEE_GWEI", MAX_FEE_GWEI)? * GWEI,
            owner: Owner {
                spend: key("VEIL_SPEND_KEY")?,
                view: key("VEIL_VIEW_KEY")?,
            },
            window: Window {
                chunk: parsed("VEIL_CHUNK", CHUNK)?,
                confirmations: parsed("VEIL_CONFIRMATIONS", 0)?,
                from: parsed("VEIL_POOL_BLOCK", 0)?,
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
    /// Named by its host only. An endpoint URL commonly carries the key
    /// that authenticates it, and an error goes to logs, crash reports and
    /// whatever is reading the terminal.
    #[error("the chain endpoint at {0} could not be reached")]
    Chain(String),
    #[error("the deposit did not land: {0}")]
    Deposit(String),
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
    #[error("the agent API could not be served: {0}")]
    Unserved(String),
}

/// The chain index, the notes we own, and the loop that keeps both current.
pub struct Daemon {
    index: Arc<IndexService>,
    wallet: Arc<WalletService>,
    envelopes: Option<Arc<EnvelopeService>>,
    api: Option<Serving>,
    retry: Retry,
}

/// The agent API, and where it answers from.
struct Serving {
    listen: SocketAddr,
    api: Api,
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
                config.fee_cap,
            )
            .await?,
        );
        let provider = ProviderBuilder::new()
            .connect(&config.rpc)
            .await
            .map_err(|_| StartError::Chain(host(&config.rpc)))?;
        let chain_id = provider
            .get_chain_id()
            .await
            .map_err(|_| StartError::Chain(host(&config.rpc)))?;
        let source = Arc::new(ChainSource::new(provider, config.pool, config.window));
        let index = Arc::new(IndexService::load(source, store.clone()).await?);

        let pool = PoolClient::new(config.pool, config.token, config.verifier, chain_id)
            .map_err(|_| StartError::Pool(config.pool))?;
        let veil = Arc::new(
            Veil::open(Proving::new(config.worker, config.artifacts))
                .map_err(|_| StartError::Artifacts)?,
        );
        let enclave = Arc::new(RemoteEnclave::new(&config.gateway, pool)?);

        let agents = Arc::new(AgentService::new(store.clone()));
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
                session: config.session,
                window: config.lock_for,
            },
        ));
        Ok(Self {
            api: Some(Serving {
                listen: config.listen,
                api: Api {
                    agents,
                    envelopes: envelopes.clone(),
                    index: index.clone(),
                    cap: config.cap,
                    started: now(),
                },
            }),
            ..Self::new(index, wallet, config.retry).serving(envelopes)
        })
    }

    /// Builds a daemon over services that are already wired.
    #[must_use]
    pub const fn new(index: Arc<IndexService>, wallet: Arc<WalletService>, retry: Retry) -> Self {
        Self {
            index,
            wallet,
            envelopes: None,
            api: None,
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
            halted = self.answering() => Err(halted),
        }
    }

    /// One pass: catch the index up, then catch the notes up with it.
    ///
    /// What is in flight advances on its own clock rather than here: this
    /// loop backs off when the endpoint is away, which is when a stuck
    /// transaction most needs rebroadcasting.
    async fn pass(&self) -> Result<(), PassError> {
        let health = self.index.sync().await?;
        let seen = self.wallet.reconcile(now()).await?;
        // A seller settles for itself when a deadline nears, so an envelope
        // can be spent without us asking. Its change is still ours.
        if let Some(envelopes) = &self.envelopes {
            envelopes.recovered().await?;
        }
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

    /// Serves the agent API, or waits forever when there is none to serve.
    async fn answering(&self) -> Halted {
        let Some(serving) = &self.api else {
            return future::pending().await;
        };
        let listener = match tokio::net::TcpListener::bind(serving.listen).await {
            Ok(listener) => listener,
            Err(error) => return Halted::Unserved(error.to_string()),
        };
        tracing::info!(listen = %serving.listen, "serving the agent API");
        match axum::serve(listener, adapters::router(serving.api.clone())).await {
            Ok(()) => future::pending().await,
            Err(error) => Halted::Unserved(error.to_string()),
        }
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
