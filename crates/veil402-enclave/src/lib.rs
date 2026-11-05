//! Metering and settlement signing for a Veil enclave.
//!
//! The enclave is the meter both sides trust: it counts what it actually sent
//! and received rather than believing a provider's reported usage, and it signs
//! a running total after every call. The seller keeps the newest signature, so
//! a buyer who disappears mid-session still pays for what it consumed.

use alloy::{
    primitives::B256,
    signers::{SignerSync, local::PrivateKeySigner},
};
use veil402_sdk::evm::{PoolClient, Session, Voucher, calls_root};

mod error;

pub use error::Error;

/// What one call cost, and the attestation that justifies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Call {
    /// Price in token units, computed by the enclave, not reported to it.
    pub cost: u64,
    /// Digest of the request, response and any provider attestation.
    pub attestation: B256,
}

/// A metered session against one reserved note.
pub struct Meter {
    pool: PoolClient,
    session: Session,
    ceiling: u64,
    total: u64,
    calls: Vec<B256>,
}

impl Meter {
    /// Opens a session. `ceiling` is what the enclave believes the reserved
    /// note holds; the circuit enforces the real bound, this only lets the
    /// enclave stop serving before a settlement would be unprovable.
    #[must_use]
    pub const fn new(pool: PoolClient, session: Session, ceiling: u64) -> Self {
        Self {
            pool,
            session,
            ceiling,
            total: 0,
            calls: Vec::new(),
        }
    }

    /// Records a served call.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CeilingReached`] when the call would take the session
    /// past what the note can pay, so the enclave stops rather than serving
    /// work nobody can settle.
    pub fn record(&mut self, call: Call) -> Result<u64, Error> {
        let total = self
            .total
            .checked_add(call.cost)
            .ok_or(Error::CeilingReached)?;
        if total > self.ceiling {
            return Err(Error::CeilingReached);
        }
        self.total = total;
        self.calls.push(call.attestation);
        Ok(total)
    }

    /// Returns the running total owed.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Returns how many calls the session has served.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.calls.len()
    }

    /// Roots the per-call attestations so one signature covers them all,
    /// whatever the call count.
    #[must_use]
    pub fn calls_root(&self) -> B256 {
        calls_root(&self.calls)
    }

    /// Signs the running total. Called after every call, or every few; the
    /// seller only ever submits the newest.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NothingOwed`] before the first call, since a
    /// settlement has to pay something.
    pub fn voucher(&self, signer: &PrivateKeySigner) -> Result<Voucher, Error> {
        if self.total == 0 {
            return Err(Error::NothingOwed);
        }
        let calls_root = self.calls_root();
        let digest = self
            .pool
            .settlement_digest(&self.session, self.total, calls_root);
        Ok(Voucher {
            session: self.session,
            amount: self.total,
            calls_root,
            signature: signer.sign_hash_sync(&digest)?.as_bytes().to_vec(),
        })
    }
}

/// Signs agreement to meter a session, so a note can only be reserved against
/// an enclave that has accepted it.
///
/// # Errors
///
/// Returns [`Error::Signature`] when signing fails.
pub fn sign_lock(
    pool: &PoolClient,
    session: &Session,
    signer: &PrivateKeySigner,
) -> Result<Vec<u8>, Error> {
    let digest = pool.lock_digest(session);
    Ok(signer.sign_hash_sync(&digest)?.as_bytes().to_vec())
}
