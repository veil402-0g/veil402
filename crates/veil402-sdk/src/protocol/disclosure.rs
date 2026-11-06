//! Proving what one payment was, to one auditor, and nothing else.
//!
//! A note's commitment is `Poseidon(npk, asset, value)` with
//! `npk = Poseidon(mpk, random)`. Poseidon binds, so a commitment opens to
//! exactly one `(mpk, random, asset, value)` — revealing that opening *is*
//! the proof, and no circuit is needed for it.
//!
//! It is also minimal. Every note is its own leaf with its own blinding, so
//! opening one says nothing about the change note beside it, the inputs it
//! came from, or any other payment. That is the difference from a viewing
//! key, which reveals every payment, forever, to whoever holds it.
//!
//! What stops a package being passed on is the signature: it covers the
//! verifier it was issued to and a nonce, so the same disclosure shown to
//! anyone else does not verify.

use alloy::{
    primitives::{Address, B256, eip191_hash_message, keccak256},
    signers::{Signature, SignerSync, local::PrivateKeySigner},
};

use super::{
    Field, Note, Owner,
    note::{commitment_from_key, note_key},
};
use crate::Error;

/// One note, opened to one verifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Disclosure {
    /// The leaf this claims to open. The verifier must also find it on
    /// chain: an opening of a commitment nobody ever made proves nothing.
    pub commitment: Field,
    /// Who the note was paid to.
    pub owner: Field,
    /// Its blinding. Revealing this opens this note and no other.
    pub random: Field,
    pub asset: Field,
    pub value: u64,
    /// Who it was issued to, and a nonce, so it cannot be handed on.
    pub verifier: Address,
    pub nonce: B256,
    pub signature: Vec<u8>,
}

impl Disclosure {
    /// Opens `note` to `verifier`, signed so it is theirs alone.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the commitment cannot be derived or the
    /// signer refuses.
    pub fn issue(
        note: &Note,
        owner: &Owner,
        signer: &PrivateKeySigner,
        verifier: Address,
        nonce: B256,
    ) -> Result<Self, Error> {
        let mut disclosure = Self {
            commitment: note.commitment(owner)?,
            owner: owner.public()?,
            random: note.random.expose(),
            asset: note.asset,
            value: note.value,
            verifier,
            nonce,
            signature: Vec::new(),
        };
        disclosure.signature = signer
            .sign_hash_sync(&eip191_hash_message(disclosure.bound()))
            .map_err(|_| Error::Transaction("the disclosure will not sign"))?
            .as_bytes()
            .to_vec();
        Ok(disclosure)
    }

    /// Checks the opening against the commitment it claims, and returns who
    /// issued it.
    ///
    /// The caller still has to find `commitment` on chain and decide whether
    /// it trusts the returned address — this says the package is internally
    /// sound and was issued to this verifier, nothing more.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transaction`] when the opening does not hash to the
    /// commitment, or the signature does not recover.
    pub fn verify(&self, verifier: Address) -> Result<Address, Error> {
        if self.verifier != verifier {
            return Err(Error::Transaction("the disclosure names another verifier"));
        }
        let derived = commitment_from_key(
            &note_key(&self.owner, &self.random)?,
            &self.asset,
            self.value,
        )?;
        if derived != self.commitment {
            return Err(Error::Transaction("the opening is not of this commitment"));
        }
        Signature::try_from(self.signature.as_slice())
            .and_then(|signature| {
                signature.recover_address_from_prehash(&eip191_hash_message(self.bound()))
            })
            .map_err(|_| Error::Transaction("the disclosure is not signed"))
    }

    /// What the signature covers: the opening, and who it is for.
    fn bound(&self) -> [u8; 32] {
        let mut bound = Vec::with_capacity(192);
        bound.extend_from_slice(&self.commitment.to_bytes());
        bound.extend_from_slice(&self.owner.to_bytes());
        bound.extend_from_slice(&self.random.to_bytes());
        bound.extend_from_slice(&self.asset.to_bytes());
        bound.extend_from_slice(&self.value.to_be_bytes());
        bound.extend_from_slice(self.verifier.as_slice());
        bound.extend_from_slice(self.nonce.as_slice());
        keccak256(bound).0
    }
}
