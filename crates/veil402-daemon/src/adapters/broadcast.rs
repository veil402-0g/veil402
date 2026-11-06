use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use alloy::{
    eips::BlockId,
    primitives::{Address, Bytes, U256},
    providers::{Provider, ProviderBuilder},
};
use async_trait::async_trait;
use veil402_sdk::Field;
use walletkit::{
    Runner, Wallet,
    adapters::{
        LocalSigner, RedbStateStore, SystemClock, Transport,
        policy::{DefaultPolicyEngine, TargetAllowlist},
    },
    core::wallet::{GasEnvelope, HandleId, TxIntent, TxStatus},
};

use crate::core::envelope::{Broadcaster, ChainError, Landed, SubmissionStore};

/// How often the sending engine is turned over: rebroadcast, confirm, bump.
/// On its own clock, because a transaction in flight needs attention
/// whether or not the index has anything to do.
const TICK: Duration = Duration::from_secs(5);

/// How long a one-off call may take to land, and how often it is asked.
const PATIENCE: Duration = Duration::from_mins(2);
const POLL: Duration = Duration::from_millis(250);

/// Sends pool calls through `walletkit`, which owns nonces, fee bumps and the
/// reorg-aware lifecycle this daemon would otherwise have to keep itself.
pub struct PoolBroadcaster<P> {
    wallet: Arc<Wallet>,
    /// Kept for the life of the broadcaster. Dropping it stops the loop
    /// that rebroadcasts, confirms and bumps — none of which can be left to
    /// the sync loop, which backs off exactly when an endpoint is away and
    /// a stuck transaction most needs attention.
    _ticking: Runner,
    provider: P,
    submissions: Arc<dyn SubmissionStore>,
    pool: Address,
    chain_id: u64,
}

/// Opens a broadcaster that may only ever call the pool.
///
/// The allowlist is not belt and braces: this key signs for a wallet whose
/// notes are the company's balance, and the pool is the only address it has
/// any business touching.
///
/// # Errors
///
/// Returns [`ChainError`] when the endpoint cannot be reached, the signing key
/// is malformed, or the transaction engine's own store cannot be opened.
pub async fn open(
    rpc: &str,
    key: &str,
    pool: Address,
    state: &std::path::Path,
    submissions: Arc<dyn SubmissionStore>,
    confirmations: u64,
    fee_cap: u128,
) -> Result<PoolBroadcaster<impl Provider + 'static>, ChainError> {
    let signer = LocalSigner::from_private_key(key)
        .map_err(|error| ChainError::Refused(error.to_string()))?;
    let policy = DefaultPolicyEngine::new(
        vec![Box::new(TargetAllowlist::new([pool]))],
        Arc::new(SystemClock),
    )
    .with_fee_caps(GasEnvelope {
        max_fee_cap: fee_cap,
        max_priority_cap: fee_cap,
    });
    let store = RedbStateStore::open(state).map_err(|_| ChainError::Unavailable)?;
    let endpoint = rpc
        .parse()
        .map_err(|_| ChainError::Refused("the endpoint is not a URL".into()))?;
    let transport = Transport::url(endpoint).map_err(|_| ChainError::Unavailable)?;
    // The same depth the index holds back by. Twelve is an Ethereum default
    // and would never arrive on a chain that reaches finality without reorgs.
    let wallet = Arc::new(
        Wallet::builder(Arc::new(transport), Arc::new(signer), Arc::new(policy))
            .store(Arc::new(store))
            .confirmations(confirmations)
            .build()
            .map_err(|_| ChainError::Unavailable)?,
    );
    let provider = ProviderBuilder::new()
        .connect(rpc)
        .await
        .map_err(|_| ChainError::Unavailable)?;
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|_| ChainError::Unavailable)?;
    Ok(PoolBroadcaster {
        _ticking: wallet.clone().run(TICK),
        wallet,
        provider,
        submissions,
        pool,
        chain_id,
    })
}

fn handle(bytes: &[u8]) -> Result<HandleId, ChainError> {
    serde_json::from_slice(bytes).map_err(|_| ChainError::Refused("unreadable handle".into()))
}

impl<P> PoolBroadcaster<P> {
    /// The address this daemon's calls come from, and whose public balance
    /// is what is left to shield.
    #[must_use]
    pub fn account(&self) -> Address {
        self.wallet.account()
    }
}

#[async_trait]
impl<P: Provider + Send + Sync> Broadcaster for PoolBroadcaster<P> {
    async fn submit(&self, key: Field, calldata: Vec<u8>) -> Result<(), ChainError> {
        if self.submissions.handle(key).await?.is_some() {
            return Ok(());
        }
        let intent = TxIntent::call(
            self.chain_id,
            self.wallet.account(),
            self.pool,
            U256::ZERO,
            Bytes::from(calldata),
        );
        let sent = self
            .wallet
            .send(&intent)
            .await
            .map_err(|error| ChainError::Refused(error.to_string()))?;
        let id = serde_json::to_vec(&sent.id)
            .map_err(|_| ChainError::Refused("unwritable handle".into()))?;
        Ok(self.submissions.track(key, &id).await?)
    }

    async fn landed(&self, key: Field) -> Result<Option<Landed>, ChainError> {
        let Some(tracked) = self.submissions.handle(key).await? else {
            return Ok(None);
        };
        let status = self
            .wallet
            .status(handle(&tracked)?)
            .await
            .map_err(|_| ChainError::Unavailable)?;
        Ok(match status {
            Some(TxStatus::Confirmed { block }) => Some(Landed::Confirmed { block }),
            Some(TxStatus::Failed { reason }) => Some(Landed::Failed(reason)),
            Some(TxStatus::Replaced | TxStatus::Dropped) => Some(Landed::Dropped),
            _ => None,
        })
    }

    async fn timestamp(&self) -> Result<u64, ChainError> {
        self.provider
            .get_block(BlockId::latest())
            .await
            .map_err(|_| ChainError::Unavailable)?
            .map(|block| block.header.timestamp)
            .ok_or(ChainError::Unavailable)
    }
}

/// Sends the one-off calls a deposit makes.
///
/// Its own engine rather than the daemon's, because approving the pool means
/// touching the token, which the daemon has no business doing. It drives the
/// engine from the loop that waits rather than from a task of its own: a
/// command that ends has to let go of the store on its last line, and a
/// background ticker would still be holding it.
pub struct Depositor {
    wallet: Arc<Wallet>,
    chain_id: u64,
}

/// Opens a sender that may only ever reach `targets`.
///
/// # Errors
///
/// Returns [`ChainError`] when the endpoint cannot be reached, the key is
/// malformed, or the engine's own store cannot be opened.
pub async fn depositor(
    rpc: &str,
    key: &str,
    targets: [Address; 2],
    state: &std::path::Path,
    fee_cap: u128,
) -> Result<Depositor, ChainError> {
    let signer = LocalSigner::from_private_key(key)
        .map_err(|error| ChainError::Refused(error.to_string()))?;
    let policy = DefaultPolicyEngine::new(
        vec![Box::new(TargetAllowlist::new(targets))],
        Arc::new(SystemClock),
    )
    .with_fee_caps(GasEnvelope {
        max_fee_cap: fee_cap,
        max_priority_cap: fee_cap,
    });
    let store = RedbStateStore::open(state).map_err(|_| {
        ChainError::Refused(
            "the transaction engine's state is already open; stop the daemon first".into(),
        )
    })?;
    let endpoint = rpc
        .parse()
        .map_err(|_| ChainError::Refused("the endpoint is not a URL".into()))?;
    let transport = Transport::url(endpoint).map_err(|_| ChainError::Unavailable)?;
    let wallet = Arc::new(
        Wallet::builder(Arc::new(transport), Arc::new(signer), Arc::new(policy))
            .store(Arc::new(store))
            .build()
            .map_err(|_| ChainError::Unavailable)?,
    );
    let chain_id = ProviderBuilder::new()
        .connect(rpc)
        .await
        .map_err(|_| ChainError::Unavailable)?
        .get_chain_id()
        .await
        .map_err(|_| ChainError::Unavailable)?;
    Ok(Depositor { wallet, chain_id })
}

impl Depositor {
    #[must_use]
    pub fn account(&self) -> Address {
        self.wallet.account()
    }

    /// Sends one call and waits for the chain to take it.
    ///
    /// # Errors
    ///
    /// Returns [`ChainError::Refused`] when the call is rejected, replaced,
    /// or never lands.
    pub async fn send(&self, to: Address, calldata: Vec<u8>) -> Result<(), ChainError> {
        let intent = TxIntent::call(
            self.chain_id,
            self.wallet.account(),
            to,
            U256::ZERO,
            Bytes::from(calldata),
        );
        let sent = self
            .wallet
            .send(&intent)
            .await
            .map_err(|error| ChainError::Refused(error.to_string()))?;
        let since = Instant::now();
        loop {
            // Nothing else is turning the engine over, so this does.
            self.wallet
                .tick()
                .await
                .map_err(|error| ChainError::Refused(error.to_string()))?;
            match self
                .wallet
                .status(sent.id)
                .await
                .map_err(|_| ChainError::Unavailable)?
            {
                Some(TxStatus::Confirmed { .. }) => return Ok(()),
                Some(TxStatus::Failed { reason }) => return Err(ChainError::Refused(reason)),
                Some(TxStatus::Replaced | TxStatus::Dropped) => {
                    return Err(ChainError::Refused("the call never landed".into()));
                }
                _ if since.elapsed() > PATIENCE => {
                    return Err(ChainError::Refused("the call did not land in time".into()));
                }
                _ => tokio::time::sleep(POLL).await,
            }
        }
    }
}
