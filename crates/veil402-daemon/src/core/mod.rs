//! Deterministic, I/O-free daemon logic.

use thiserror::Error;

pub mod agent;
pub mod envelope;
pub mod index;
pub mod wallet;

/// Why a store could not answer. Shared: both slices sit on one database.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum StoreError {
    #[error("the store is unavailable")]
    Unavailable,
    #[error("the store holds a gap or a duplicate")]
    Corrupt,
}
