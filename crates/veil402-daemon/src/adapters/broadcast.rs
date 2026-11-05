use std::{sync::Arc, time::Duration};

use alloy::{
    eips::BlockId,
    primitives::{Address, Bytes, U256},
    providers::{Provider, ProviderBuilder},
};
use async_trait::async_trait;
use tokio::time::sleep;
use walletkit::{
    Wallet,
    adapters::{
        LocalSigner, RedbStateStore, SystemClock, Transport,
        policy::{DefaultPolicyEngine, TargetAllowlist},
    },
    core::wallet::{TxIntent, TxStatus},
};

use crate::core::envelope::{Broadcaster, ChainError};

/// How often the lifecycle is advanced while a call is in flight.
const TICK: Duration = Duration::from_millis(500);

/// Sends pool calls through `walletkit`, which owns nonces, fee bumps and the
/// reorg-aware lifecycle this daemon would otherwise have to keep itself.
pub struct PoolBroadcaster<P> {
    wallet: Wallet,
    provider: P,
    pool: Address,
    chain_id: u64,
}

impl PoolBroadcaster<()> {
    /// Builds a broadcaster that may only ever call the pool.
    ///
    /// The allowlist is not belt and braces: this key signs for a wallet whose
    /// notes are the company's balance, and the pool is the only address it
    /// has any business touching.
    ///
    /// # Errors
    ///
    /// Returns [`ChainError::Unavailable`] when the endpoint cannot be reached
    /// or the signing key is malformed.
    pub async fn open(
        rpc: &str,
        key: &str,
        pool: Address,
        state: &std::path::Path,
    ) -> Result<PoolBroadcaster<impl Provider>, ChainError> {
        let signer = LocalSigner::from_private_key(key)
            .map_err(|error| ChainError::Refused(error.to_string()))?;
        let policy = DefaultPolicyEngine::new(
            vec![Box::new(TargetAllowlist::new([pool]))],
            Arc::new(SystemClock),
        );
        let store = RedbStateStore::open(state).map_err(|_| ChainError::Unavailable)?;
        let endpoint = rpc
            .parse()
            .map_err(|_| ChainError::Refused("the endpoint is not a URL".into()))?;
        let transport = Transport::url(endpoint).map_err(|_| ChainError::Unavailable)?;
        let wallet = Wallet::builder(Arc::new(transport), Arc::new(signer), Arc::new(policy))
            .store(Arc::new(store))
            .build()
            .map_err(|_| ChainError::Unavailable)?;
        let provider = ProviderBuilder::new()
            .connect(rpc)
            .await
            .map_err(|_| ChainError::Unavailable)?;
        let chain_id = provider
            .get_chain_id()
            .await
            .map_err(|_| ChainError::Unavailable)?;
        Ok(PoolBroadcaster {
            wallet,
            provider,
            pool,
            chain_id,
        })
    }
}

#[async_trait]
impl<P: Provider + Send + Sync> Broadcaster for PoolBroadcaster<P> {
    async fn send(&self, calldata: Vec<u8>) -> Result<(), ChainError> {
        let intent = TxIntent::call(
            self.chain_id,
            self.wallet.account(),
            self.pool,
            U256::ZERO,
            Bytes::from(calldata),
        );
        let handle = self
            .wallet
            .send(&intent)
            .await
            .map_err(|error| ChainError::Refused(error.to_string()))?;

        loop {
            self.wallet
                .tick()
                .await
                .map_err(|_| ChainError::Unavailable)?;
            match self.wallet.status(handle.id).await {
                Ok(Some(TxStatus::Confirmed { .. })) => return Ok(()),
                Ok(Some(TxStatus::Failed { reason })) => return Err(ChainError::Refused(reason)),
                // A replaced or dropped call never landed, and the caller's
                // next step assumed it had.
                Ok(Some(TxStatus::Replaced)) => {
                    return Err(ChainError::Refused("replaced at the same nonce".into()));
                }
                Ok(Some(TxStatus::Dropped)) => {
                    return Err(ChainError::Refused("evicted before inclusion".into()));
                }
                Ok(_) => sleep(TICK).await,
                Err(_) => return Err(ChainError::Unavailable),
            }
        }
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
