use thiserror::Error;

use crate::core::StoreError;

/// Why a transaction could not be submitted or tracked.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ChainError {
    #[error("the chain is unreachable")]
    Unavailable,
    /// The pool refused the call. Sending it again changes nothing: a deadline
    /// has passed, a nullifier is taken, or a signature does not recover.
    #[error("the pool refused the call: {0}")]
    Refused(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl ChainError {
    /// Whether trying again could plausibly succeed.
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        matches!(self, Self::Unavailable | Self::Store(StoreError::Unavailable))
    }
}

/// Why a proof could not be produced.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProveError {
    /// The transaction breaks a protocol invariant, so no proof exists.
    #[error("the transaction cannot be proved: {0}")]
    Invalid(&'static str),
    #[error("the prover failed")]
    Failed,
}

impl ProveError {
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        matches!(self, Self::Failed)
    }
}
