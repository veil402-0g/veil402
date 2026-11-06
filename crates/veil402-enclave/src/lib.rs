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
use veil402_sdk::{
    MerklePath, Note, Prepared, Send, SessionKeys, Spend, Transaction, Veil, change_random,
    evm::{PoolClient, Session, Voucher, calls_root},
    session::{EnclaveKey, Handover, Sealed},
    split,
};

mod error;
mod journal;

pub use error::Error;
pub use journal::{Recorded, record, replay};
pub use veil402_sdk::evm::Call;

/// What a settlement pays when nothing was served, so the note comes back
/// rather than waiting out the deadline.
const DUST: u64 = 1;

/// A metered session against one reserved note.
pub struct Meter {
    pool: PoolClient,
    session: Session,
    handover: Handover,
    total: u64,
    calls: Vec<B256>,
}

impl Meter {
    /// Accepts a delegation, or refuses it.
    ///
    /// The only constructor, so a session cannot be metered before the seed it
    /// was handed has been checked against the escrow it agreed to meter. The
    /// ceiling is the note's own value rather than something the enclave is
    /// told, so it cannot be asked to serve past what a settlement could pay.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Refused`] when the envelope does not open, the seed
    /// opens another note, or the delegation names another pool.
    pub fn open(
        pool: PoolClient,
        key: &EnclaveKey,
        sealed: &Sealed,
        session: Session,
    ) -> Result<Self, Error> {
        let handover = Handover::from_bytes(&key.open(sealed)?)?;
        handover.accept(&session)?;
        if handover.pool != pool.address() || handover.chain_id != pool.chain_id() {
            return Err(Error::Refused("delegation names another pool"));
        }
        Ok(Self {
            pool,
            session,
            handover,
            total: 0,
            calls: Vec::new(),
        })
    }

    /// Rebuilds a session from the records this enclave sealed.
    ///
    /// The host keeps them because a Nitro enclave has no disk, and it is not
    /// trusted with them: a total it invented does not open, so the only
    /// thing it can do with the log is lose part of it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Refused`] when the delegation does not open onto the
    /// escrow, or a record was not sealed by this enclave.
    pub fn resume(
        pool: PoolClient,
        key: &EnclaveKey,
        sealed: &Sealed,
        session: Session,
        log: &[Vec<u8>],
    ) -> Result<(Self, Vec<journal::Recorded>), Error> {
        let replayed = journal::replay(key, log)?;
        let mut meter = Self::open(pool, key, sealed, session)?;
        meter.total = replayed.last().map_or(0, |record| record.total);
        meter.calls = replayed
            .iter()
            .map(|record| record.attestation)
            .collect();
        Ok((meter, replayed))
    }

    /// The escrow this session settles against.
    #[must_use]
    pub const fn session(&self) -> &Session {
        &self.session
    }

    /// The tree the delegation claims its note sits in.
    ///
    /// The path was checked against it here; whether the pool ever reached it
    /// is a question only something with a chain can answer.
    #[must_use]
    pub const fn root(&self) -> veil402_sdk::Field {
        self.handover.root
    }

    /// What the reserved note holds, and so the most this session can cost.
    #[must_use]
    pub const fn ceiling(&self) -> u64 {
        self.handover.value
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
        if total > self.ceiling() {
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
        self.voucher_at(self.total, signer)
    }

    fn voucher_at(&self, amount: u64, signer: &PrivateKeySigner) -> Result<Voucher, Error> {
        let calls_root = self.calls_root();
        let digest = self.pool.settlement_digest(&self.session, amount, calls_root);
        Ok(Voucher {
            session: self.session,
            amount,
            calls_root,
            signature: signer.sign_hash_sync(&digest)?.as_bytes().to_vec(),
        })
    }
}

impl Meter {
    /// Proves and encodes the settlement, for whoever broadcasts it.
    ///
    /// The enclave holds everything a settlement needs — the note's secrets
    /// arrived in the delegation and the path never expires — so the buyer is
    /// not required to be present once the lock has landed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NothingOwed`] before the first call, or
    /// [`Error::Veil`] when the transaction cannot be built or proved.
    pub async fn settle(&self, veil: &Veil, signer: &PrivateKeySigner) -> Result<Prepared, Error> {
        if self.total == 0 {
            return Err(Error::NothingOwed);
        }
        self.prove(self.total, veil, signer).await
    }

    /// Settles for a unit so the note comes back now.
    ///
    /// The pool has no way to cancel a reservation, so a seller that leaves
    /// having served nothing would otherwise strand the buyer's capital until
    /// the deadline. A settlement must pay something, and a unit is what that
    /// costs.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Veil`] when the transaction cannot be built or proved.
    pub async fn release(&self, veil: &Veil, signer: &PrivateKeySigner) -> Result<Prepared, Error> {
        self.prove(self.total.max(DUST), veil, signer).await
    }

    async fn prove(
        &self,
        amount: u64,
        veil: &Veil,
        signer: &PrivateKeySigner,
    ) -> Result<Prepared, Error> {
        let voucher = self.voucher_at(amount, signer)?;
        let keys = SessionKeys::from_seed(&self.handover.seed)?;
        let spend = Spend {
            note: Note {
                value: self.handover.value,
                asset: self.handover.asset,
                random: keys.random,
            },
            owner: keys.owner,
            merkle: MerklePath {
                index: self.handover.leaf,
                siblings: self.handover.siblings,
            },
        };
        let transaction = Transaction::new(vec![spend], self.outputs_at(amount)?)?;
        Ok(veil.settle(&self.pool, transaction, voucher).await?)
    }

    /// The notes a settlement leaves behind: a session-sized one for the next
    /// envelope, then the remainder.
    ///
    /// Both derive from `change_seed`, and the amount is public, so a buyer
    /// that was away reconstructs them without a ciphertext to decrypt.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Veil`] when a note cannot be derived.
    pub fn outputs(&self) -> Result<Vec<Send>, Error> {
        self.outputs_at(self.total)
    }

    fn outputs_at(&self, amount: u64) -> Result<Vec<Send>, Error> {
        let change = self.handover.value.saturating_sub(amount);
        let mut amounts = split(change, self.handover.target).into_iter();
        let mut sends = Vec::new();
        if let Some(amount) = amounts.next() {
            let keys = SessionKeys::from_seed(&self.handover.change_seed)?;
            sends.push(Send::new(
                keys.owner.public()?,
                amount,
                keys.random,
                Vec::new(),
            )?);
        }
        if let Some(amount) = amounts.next() {
            sends.push(Send::new(
                self.handover.change_owner,
                amount,
                change_random(&self.handover.change_seed)?,
                Vec::new(),
            )?);
        }
        Ok(sends)
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
