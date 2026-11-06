use async_trait::async_trait;
use futures_util::stream::BoxStream;
use thiserror::Error;
use veil402_sdk::{
    Field, Prepared, Secret, Transaction,
    evm::{Session, Voucher},
    session::Sealed,
    wire::Offer,
};

use super::{Agreement, ChainError, Piece, ProveError, Settlement};
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

/// Why the enclave would not act.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum EnclaveError {
    /// The enclave does not hold this session. After a restart it has to be
    /// handed its delegation again before it can meter anything.
    #[error("the enclave does not hold this session")]
    Unknown,
    /// Serving this call would take the envelope past what its note can pay.
    #[error("the envelope's ceiling is reached")]
    CeilingReached,
    /// The seller's upstream refused, in its own words. They came back
    /// sealed, so the host that relayed them never read them.
    #[error("the upstream refused with {status}")]
    Upstream {
        status: u16,
        retry_after: Option<u64>,
        body: Vec<u8>,
    },
    #[error("the enclave refused: {0}")]
    Refused(String),
}

impl EnclaveError {
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        false
    }
}

/// The meter both sides trust.
///
/// A trait because the enclave runs in this process today and behind a vsock
/// once there is hardware: that move must change one adapter and nothing else.
#[async_trait]
pub trait Enclave: Send + Sync {
    /// What this seller sells, and what it charges for it.
    async fn catalogue(&self) -> Result<Vec<Offer>, EnclaveError>;

    /// Agrees to meter a session, returning the signatures and the key its
    /// delegation must be sealed to.
    async fn agree(&self, session: &Session) -> Result<Agreement, EnclaveError>;

    /// Takes the sealed delegation. The enclave checks it against the escrow
    /// before it will meter anything.
    async fn accept(&self, sealed: Sealed, session: Session) -> Result<(), EnclaveError>;

    /// Sends the agent's request, and returns the answer to it.
    ///
    /// `body` is the agent's own bytes. Sealing them, and minting the
    /// channel the answer comes back over, belong to whichever adapter is
    /// in use — so the host that relays it learns which session is working
    /// and what it cost, never what was asked. `attempt` names the call
    /// rather than the retry, so one served twice is billed once.
    async fn chat(
        &self,
        nullifier: Field,
        body: &[u8],
        attempt: &str,
    ) -> Result<BoxStream<'static, Result<Piece, EnclaveError>>, EnclaveError>;

    /// Proves the settlement. `dust` asks for the smallest one that frees the
    /// note, for an envelope nobody owes anything on.
    async fn settle(&self, nullifier: Field, dust: bool) -> Result<Settlement, EnclaveError>;
}

/// Holds the one secret an envelope cannot derive again.
///
/// Everything else about an envelope is on chain. This is not: without it the
/// change a settlement mints can never be identified, let alone spent.
#[async_trait]
pub trait EnvelopeStore: Send + Sync {
    async fn remember(&self, nullifier: Field, change_seed: &Secret) -> Result<(), StoreError>;

    async fn change_seed(&self, nullifier: Field) -> Result<Option<Secret>, StoreError>;

    /// The last envelope opened.
    ///
    /// One seller and one shared ceiling mean one envelope serves whatever
    /// is asked of it, whichever model that turns out to be. Seller,
    /// deadline and ceiling are all on chain against the nullifier, and a
    /// second copy of a fact the chain decides is one that can disagree.
    async fn newest(&self) -> Result<Option<Field>, StoreError>;
}
