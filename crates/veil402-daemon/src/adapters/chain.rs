use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::Filter,
    sol,
    sol_types::SolEvent,
};
use async_trait::async_trait;
use veil402_sdk::Field;

use crate::core::index::{Batch, ChainError, Commitment, CommitmentSource, Nullified};

sol! {
    #[sol(rpc)]
    interface IPool {
        event NewCommitment(uint256 indexed commitment, uint256 leafIndex, bytes note);
        event Nullified(uint256 indexed nullifier);
        function currentRoot() external view returns (uint256);
        function knownRoot(uint256 root) external view returns (bool);
    }
}

/// Events one pass may carry, so a long history is applied as a series of
/// transactions rather than as one.
const BATCH: usize = 10_000;

/// How far the source reads in one pass.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// Blocks per query, since relays cap how many one may span.
    pub chunk: u64,
    /// Blocks held back from the head. Hedera reaches finality without reorgs,
    /// so zero is correct there.
    pub confirmations: u64,
}

/// Reads the pool's events through an EVM endpoint.
pub struct ChainSource<P> {
    provider: P,
    pool: Address,
    window: Window,
}

impl<P: Provider> ChainSource<P> {
    /// Binds a source to a deployed pool.
    #[must_use]
    pub const fn new(provider: P, pool: Address, window: Window) -> Self {
        Self {
            provider,
            pool,
            window,
        }
    }
}

fn field(value: U256) -> Result<Field, ChainError> {
    Field::from_bytes(value.to_be_bytes()).map_err(|_| ChainError::InvalidData)
}

#[async_trait]
impl<P: Provider + Send + Sync> CommitmentSource for ChainSource<P> {
    async fn since(&self, block: u64) -> Result<Batch, ChainError> {
        let head = self
            .provider
            .get_block_number()
            .await
            .map_err(|_| ChainError::Unavailable)?
            .saturating_sub(self.window.confirmations);

        let mut batch = Batch::default();
        let mut from = block.saturating_add(1);
        while from <= head {
            let to = from
                .saturating_add(self.window.chunk.saturating_sub(1))
                .min(head);
            let logs = self
                .provider
                .get_logs(
                    &Filter::new()
                        .address(self.pool)
                        .events([IPool::NewCommitment::SIGNATURE, IPool::Nullified::SIGNATURE])
                        .from_block(from)
                        .to_block(to),
                )
                .await
                .map_err(|_| ChainError::Unavailable)?;

            for log in logs {
                let at = log.block_number.ok_or(ChainError::InvalidData)?;
                if log.topic0() == Some(&IPool::NewCommitment::SIGNATURE_HASH) {
                    let event = log
                        .log_decode::<IPool::NewCommitment>()
                        .map_err(|_| ChainError::InvalidData)?;
                    let data = event.inner.data;
                    batch.commitments.push(Commitment {
                        leaf_index: u32::try_from(data.leafIndex)
                            .map_err(|_| ChainError::InvalidData)?,
                        commitment: field(data.commitment)?,
                        note: data.note.to_vec(),
                        block: at,
                    });
                } else {
                    let event = log
                        .log_decode::<IPool::Nullified>()
                        .map_err(|_| ChainError::InvalidData)?;
                    batch.nullifiers.push(Nullified {
                        nullifier: field(event.inner.data.nullifier)?,
                        block: at,
                    });
                }
            }
            batch.through = to;
            from = to.saturating_add(1);
            if batch.commitments.len() + batch.nullifiers.len() >= BATCH {
                break;
            }
        }

        // Logs arrive per chunk; the tree only accepts them in insertion order.
        batch.commitments.sort_unstable_by_key(|held| held.leaf_index);
        Ok(batch)
    }

    async fn is_known_root(&self, root: Field) -> Result<bool, ChainError> {
        IPool::new(self.pool, &self.provider)
            .knownRoot(U256::from_be_bytes(root.to_bytes()))
            .call()
            .await
            .map_err(|_| ChainError::Unavailable)
    }

    async fn root(&self) -> Result<Field, ChainError> {
        let root = IPool::new(self.pool, &self.provider)
            .currentRoot()
            .call()
            .await
            .map_err(|_| ChainError::Unavailable)?;
        field(root)
    }
}
