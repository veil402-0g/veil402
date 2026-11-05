use async_trait::async_trait;
use thiserror::Error;
use veil402_sdk::{Field, StoredNode};

use super::{Batch, Commitment};
use crate::core::StoreError;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ChainError {
    #[error("the chain is unreachable")]
    Unavailable,
    /// The pool emitted something this build cannot read, or a log arrived
    /// without the block that carried it.
    #[error("the chain returned an event the pool could not have emitted")]
    InvalidData,
}

/// Reads what the pool has emitted.
#[async_trait]
pub trait CommitmentSource: Send + Sync {
    /// Everything after `block`, in insertion order, bounded so that one pass
    /// stays within one transaction.
    async fn since(&self, block: u64) -> Result<Batch, ChainError>;

    /// Whether the pool has ever reached this root. This is the test the pool
    /// itself applies when a proof arrives, so passing it means the root will
    /// not be the reason a spend is rejected.
    async fn is_known_root(&self, root: Field) -> Result<bool, ChainError>;

    /// The pool's latest root, for reporting how far behind the index is.
    async fn root(&self) -> Result<Field, ChainError>;
}

/// Holds the index across restarts. The chain only answers a fixed window, so
/// losing this cannot be recovered by resyncing.
#[async_trait]
pub trait IndexStore: Send + Sync {
    /// The last block applied.
    async fn cursor(&self) -> Result<u64, StoreError>;

    /// Every tree node, in level and index order. Restoring from these is what
    /// lets the tree be loaded without hashing its whole history again.
    async fn nodes(&self) -> Result<Vec<StoredNode>, StoreError>;

    /// Events, nodes and the cursor move together or not at all: a crash
    /// between them would replay leaves at shifted indices.
    async fn append(&self, batch: &Batch, nodes: &[StoredNode]) -> Result<(), StoreError>;

    /// The leaf a commitment landed at, for a payee confirming its note is in
    /// the tree.
    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError>;

    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError>;

    /// Commitments from `from`, for scanning ciphertexts to find owned notes.
    async fn notes(&self, from: u32, limit: u32) -> Result<Vec<Commitment>, StoreError>;
}
