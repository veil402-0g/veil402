use std::num::NonZeroU64;

use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::{Filter, Log},
    sol,
    sol_types::SolEvent,
};
use async_trait::async_trait;
use veil402_sdk::Field;

use crate::core::index::{
    Batch, ChainError, Commitment, CommitmentSource, Funding, Locked, Nullified, Settled,
};

sol! {
    #[sol(rpc)]
    interface IPool {
        event NewCommitment(uint256 indexed commitment, uint256 leafIndex, bytes note);
        event Nullified(uint256 indexed nullifier);
        event Settled(uint256 indexed nullifier, bytes32 callsRoot, uint256 amount);
        event Locked(
            uint256 indexed nullifier,
            address indexed enclave,
            address indexed seller,
            uint64 deadline
        );
        function currentRoot() external view returns (uint256);
        function knownRoot(uint256 root) external view returns (bool);
    }
}

/// Events one pass may carry, so a long history is applied as a series of
/// transactions rather than as one. Checked between chunks, so a single chunk
/// can overshoot it; what it bounds is how far one pass walks.
const BATCH: usize = 10_000;

/// How far the source reads in one pass.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// Blocks per query, since relays cap how many one may span. Non-zero
    /// because a zero chunk would crawl the chain a block at a time rather
    /// than fail.
    pub chunk: NonZeroU64,
    /// Blocks held back from the head. Hedera reaches finality without
    /// reorgs, so zero is correct there.
    pub confirmations: u64,
    /// Block the pool was deployed in. A pool emits nothing before it
    /// existed, and on a chain that is already millions of blocks deep,
    /// walking up to it is tens of thousands of queries that can only
    /// return nothing.
    pub from: u64,
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

    /// The newest block this source will read from, held back from the head by
    /// the configured confirmations.
    async fn head(&self) -> Result<u64, ChainError> {
        self.provider
            .get_block_number()
            .await
            .inspect_err(|error| tracing::warn!(%error, "block number unavailable"))
            .map(|head| head.saturating_sub(self.window.confirmations))
            .map_err(|_| ChainError::Unavailable)
    }

    /// One chunk of the pool's logs.
    async fn logs(&self, from: u64, to: u64) -> Result<Vec<Log>, ChainError> {
        self.provider
            .get_logs(
                &Filter::new()
                    .address(self.pool)
                    .events([
                        IPool::NewCommitment::SIGNATURE,
                        IPool::Nullified::SIGNATURE,
                        IPool::Locked::SIGNATURE,
                        IPool::Settled::SIGNATURE,
                    ])
                    .from_block(from)
                    .to_block(to),
            )
            .await
            .inspect_err(|error| tracing::warn!(%error, from, to, "log query failed"))
            .map_err(|_| ChainError::Unavailable)
    }

    fn contract(&self) -> IPool::IPoolInstance<&P> {
        IPool::new(self.pool, &self.provider)
    }
}

fn decode<E: SolEvent>(log: &Log) -> Result<E, ChainError> {
    log.log_decode::<E>()
        .map(|decoded| decoded.inner.data)
        .inspect_err(|error| tracing::warn!(%error, "undecodable log"))
        .map_err(|_| ChainError::InvalidData)
}

fn field(value: U256) -> Result<Field, ChainError> {
    Field::from_bytes(value.to_be_bytes()).map_err(|_| ChainError::InvalidData)
}

/// Files one log under the event it is.
fn absorb(batch: &mut Batch, log: &Log) -> Result<(), ChainError> {
    let block = log.block_number.ok_or(ChainError::InvalidData)?;
    match log.topic0() {
        Some(&IPool::NewCommitment::SIGNATURE_HASH) => {
            let event = decode::<IPool::NewCommitment>(log)?;
            batch.commitments.push(Commitment {
                leaf_index: u32::try_from(event.leafIndex).map_err(|_| ChainError::InvalidData)?,
                commitment: field(event.commitment)?,
                note: event.note.to_vec(),
                block,
            });
        }
        Some(&IPool::Nullified::SIGNATURE_HASH) => batch.nullifiers.push(Nullified {
            nullifier: field(decode::<IPool::Nullified>(log)?.nullifier)?,
            block,
        }),
        Some(&IPool::Locked::SIGNATURE_HASH) => {
            let event = decode::<IPool::Locked>(log)?;
            batch.locks.push(Locked {
                nullifier: field(event.nullifier)?,
                seller: event.seller,
                enclave: event.enclave,
                deadline: event.deadline,
                block,
            });
        }
        Some(&IPool::Settled::SIGNATURE_HASH) => {
            let event = decode::<IPool::Settled>(log)?;
            batch.settlements.push(Settled {
                nullifier: field(event.nullifier)?,
                amount: u64::try_from(event.amount).map_err(|_| ChainError::InvalidData)?,
                block,
            });
        }
        // The filter asks for four topics. A fifth means this ABI no longer
        // describes the pool, and guessing past that would miss commitments.
        topic => {
            tracing::error!(?topic, "the pool emitted an event this build cannot read");
            return Err(ChainError::InvalidData);
        }
    }
    Ok(())
}

#[async_trait]
impl<P: Provider + Send + Sync> CommitmentSource for ChainSource<P> {
    async fn since(&self, block: u64) -> Result<Batch, ChainError> {
        let head = self.head().await?;
        let mut batch = Batch::default();
        let mut from = block.saturating_add(1).max(self.window.from);

        while from <= head && batch.len() < BATCH {
            let to = from
                .saturating_add(self.window.chunk.get() - 1)
                .min(head);
            for log in self.logs(from, to).await? {
                absorb(&mut batch, &log)?;
            }
            batch.through = to;
            from = to.saturating_add(1);
        }
        if from <= head {
            tracing::debug!(
                events = batch.len(),
                through = batch.through,
                head,
                "pass truncated, still catching up"
            );
        }

        // Logs arrive per chunk; the tree only accepts them in insertion order.
        batch.commitments.sort_unstable_by_key(|note| note.leaf_index);
        Ok(batch)
    }

    async fn is_known_root(&self, root: Field) -> Result<bool, ChainError> {
        self.contract()
            .knownRoot(U256::from_be_bytes(root.to_bytes()))
            .call()
            .await
            .inspect_err(|error| tracing::warn!(%error, "knownRoot unavailable"))
            .map_err(|_| ChainError::Unavailable)
    }

    async fn root(&self) -> Result<Field, ChainError> {
        let root = self
            .contract()
            .currentRoot()
            .call()
            .await
            .inspect_err(|error| tracing::warn!(%error, "currentRoot unavailable"))
            .map_err(|_| ChainError::Unavailable)?;
        field(root)
    }
}

sol! {
    #[sol(rpc)]
    interface IFunds {
        function balanceOf(address owner) external view returns (uint256);
    }
}

/// Reads the public balance of the key that shields funds.
pub struct Purse<P> {
    provider: P,
    token: Address,
    owner: Address,
}

impl<P: Provider> Purse<P> {
    pub const fn new(provider: P, token: Address, owner: Address) -> Self {
        Self {
            provider,
            token,
            owner,
        }
    }
}

#[async_trait]
impl<P: Provider + Send + Sync> Funding for Purse<P> {
    async fn depositable(&self) -> Result<u64, ChainError> {
        let held = IFunds::new(self.token, &self.provider)
            .balanceOf(self.owner)
            .call()
            .await
            .map_err(|_| ChainError::Unavailable)?;
        // A balance past what a note can hold is one the pool would refuse
        // anyway; reporting the cap is truer than overflowing.
        Ok(u64::try_from(held).unwrap_or(u64::MAX))
    }
}
