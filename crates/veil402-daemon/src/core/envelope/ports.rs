use async_trait::async_trait;
use veil402_sdk::{Prepared, Transaction, evm::Voucher};

use super::{ChainError, ProveError};

/// Sends calls to the pool, and reads the clock they are judged against.
#[async_trait]
pub trait Broadcaster: Send + Sync {
    /// Submits pool calldata and returns once it is final.
    ///
    /// Blocking is the honest shape: a note cannot be witnessed before its
    /// commitment is indexed, and a settlement cannot be built before its lock
    /// has landed, so nothing the daemon does next can start any earlier.
    async fn send(&self, calldata: Vec<u8>) -> Result<(), ChainError>;

    /// The latest block's timestamp.
    ///
    /// A lock's deadline is checked against this, never the host clock,
    /// because a few seconds of drift is the difference between a lock and a
    /// `BadDeadline` revert.
    async fn timestamp(&self) -> Result<u64, ChainError>;
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
