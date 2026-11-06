use alloy::primitives::{Address, B256};
use veil402_sdk::Field;

/// A note reserved against one seller, for one ceiling, until one deadline.
///
/// The unit of settlement. A session that runs for hours is a chain of these,
/// because a lock cannot outlive `MAX_LOCK` and a ceiling cannot outlive its
/// note.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Envelope {
    /// What the pool reserved, and the key its lock was submitted under.
    pub nullifier: Field,
    /// The note behind it.
    pub commitment: Field,
    /// Who the enclave may pay.
    pub seller: Address,
    /// Last second the seller may settle.
    pub deadline: u64,
    /// The note's value, which is the most this envelope can cost.
    pub ceiling: u64,
}

/// Where an envelope stands, derived from the chain rather than stored.
///
/// Storing it would mean two records of the same fact, and the chain's is the
/// one that decides.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// The lock has been submitted but the pool has not recorded it.
    Locking,
    /// Reserved, and far enough from the deadline to keep serving.
    Open,
    /// Close enough to the deadline that settling has to start now.
    Expiring,
    /// The pool has spent the nullifier.
    Settled,
}

/// What an enclave returns when it agrees to meter a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agreement {
    /// The registered key the pool will recover the lock signature to.
    pub signer: Address,
    /// The X25519 key this envelope's delegation is sealed to.
    pub handover_key: [u8; 32],
    /// Vouches for `handover_key`, recovering to `signer`.
    pub announcement: Vec<u8>,
    /// Consent to meter the session, for the pool's `lock`.
    pub lock: Vec<u8>,
}

/// What one metered call cost, and what proves it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metered {
    /// Cumulative cost of the envelope so far.
    pub total: u64,
    /// What is left before the ceiling stops the session.
    pub remaining: u64,
    /// Digest binding this call, for the daemon to file against the agent
    /// that made it. The enclave meters one total and never learns who asked.
    pub attestation: B256,
}

/// A settlement, as much of it as the sender needs.
///
/// Not the proof: the chain verifies that, and the daemon only broadcasts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settlement {
    /// What the submission is tracked under — an output, since the lock
    /// already used the nullifier.
    pub key: Field,
    pub calldata: Vec<u8>,
}
