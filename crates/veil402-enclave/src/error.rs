use thiserror::Error;

/// Error returned by enclave metering.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The call would take the session past what its note can pay.
    #[error("session ceiling reached")]
    CeilingReached,

    /// A settlement has to pay something.
    #[error("nothing owed yet")]
    NothingOwed,

    /// The signing key rejected the digest.
    #[error("signing failed")]
    Signature(#[from] alloy::signers::Error),
}
