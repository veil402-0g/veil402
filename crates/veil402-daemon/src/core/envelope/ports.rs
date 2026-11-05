use async_trait::async_trait;
use veil402_sdk::{Field, Prepared, Transaction, evm::Voucher};

use super::{ChainError, ProveError};
use crate::core::StoreError;

/// Where a submitted call ended up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Landed {
    Confirmed { block: u64 },
    /// The pool rejected it on chain.
    Failed(String),
    /// It never landed: replaced at the same nonce, or evicted before
    /// inclusion. Whatever the caller expected to follow it has not happened.
    Dropped,
}

/// Sends calls to the pool and tracks them to a terminal state.
///
/// Submitting and confirming are separate because only some callers need to
/// wait: a lock must land before its handover can be sealed, while a rollover
/// settlement is sent while the current envelope is still serving.
#[async_trait]
pub trait Broadcaster: Send + Sync {
    /// Submits pool calldata under a key the caller already holds — the
    /// nullifier for a lock or a settlement, the commitment for a split.
    ///
    /// Idempotent on that key, which is what makes a crash safe to retry: a
    /// resubmission adopts the call already in flight instead of spending a
    /// second nonce sending it twice.
    async fn submit(&self, key: Field, calldata: Vec<u8>) -> Result<(), ChainError>;

    /// Where a submitted call has reached, or `None` while it is in flight.
    async fn landed(&self, key: Field) -> Result<Option<Landed>, ChainError>;

    /// Advances everything in flight: rebroadcasts, bumps fees, notices
    /// reorgs. Nothing reaches a terminal state without it.
    async fn tick(&self) -> Result<(), ChainError>;

    /// The latest block's timestamp.
    ///
    /// A lock's deadline is checked against this, never the host clock,
    /// because a few seconds of drift is the difference between a lock and a
    /// `BadDeadline` revert.
    async fn timestamp(&self) -> Result<u64, ChainError>;
}

/// Remembers which call is in flight for which key.
///
/// The transaction engine keeps its own durable state and rebroadcasts from
/// it; this is the daemon's side of that — without it a restart cannot tell
/// whether its lock ever landed.
#[async_trait]
pub trait SubmissionStore: Send + Sync {
    /// Records the opaque handle a submission is tracked by.
    async fn track(&self, key: Field, handle: &[u8]) -> Result<(), StoreError>;

    /// The handle for a key, if one was ever submitted.
    async fn handle(&self, key: Field) -> Result<Option<Vec<u8>>, StoreError>;
}

/// Produces the proofs the pool verifies.
#[async_trait]
pub trait Prover: Send + Sync {
    /// Proves a shielded transfer, which is how one note becomes two.
    async fn transfer(&self, transaction: Transaction) -> Result<Prepared, ProveError>;

    /// Proves a settlement against a reserved note.
    async fn settle(
        &self,
        transaction: Transaction,
        voucher: Voucher,
    ) -> Result<Prepared, ProveError>;
}
