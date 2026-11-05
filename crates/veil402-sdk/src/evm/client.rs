use alloy::{
    primitives::{Address, Bytes, U256},
    sol_types::SolCall,
};

use super::{
    Pool,
    asset::asset_id,
    binding::{External, ext_data_hash},
    calldata::ProofCalldata,
};
use crate::{
    Field,
    client::{Error, Proof},
};

/// Immutable configuration for one deployed pool.
///
/// Every value the pool binds into a proof lives here, so a proof cannot be
/// built against one deployment and submitted to another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PoolClient {
    address: Address,
    token: Address,
    verifier: Address,
    chain_id: u64,
    asset: Field,
}

impl PoolClient {
    /// Creates a pool configuration and derives its circuit asset.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Poseidon`] if the asset derivation rejects its inputs.
    pub fn new(
        address: Address,
        token: Address,
        verifier: Address,
        chain_id: u64,
    ) -> Result<Self, Error> {
        Ok(Self {
            address,
            token,
            verifier,
            chain_id,
            asset: asset_id(token)?,
        })
    }

    /// Returns the pool address.
    #[must_use]
    pub const fn address(&self) -> Address {
        self.address
    }

    /// Returns the pooled token.
    #[must_use]
    pub const fn token(&self) -> Address {
        self.token
    }

    /// Returns the verifier the pool calls.
    #[must_use]
    pub const fn verifier(&self) -> Address {
        self.verifier
    }

    /// Returns the chain the pool is deployed on.
    #[must_use]
    pub const fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Returns the circuit asset identifier for this pool's token.
    #[must_use]
    pub const fn asset(&self) -> Field {
        self.asset
    }

    /// Encodes a public deposit of `amount` committed to `npk`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Withdrawal`] when the amount is zero or the ciphertext
    /// exceeds the pool's limit.
    pub fn shield(&self, npk: Field, amount: u64, note: &[u8]) -> Result<Vec<u8>, Error> {
        if amount == 0 {
            return Err(Error::Withdrawal("amount must be greater than zero"));
        }
        if note.len() > crate::MAX_NOTE_BYTES {
            return Err(Error::Withdrawal("ciphertext exceeds the pool limit"));
        }
        Ok(Pool::shieldCall {
            npk: word(npk),
            amount,
            note: Bytes::copy_from_slice(note),
        }
        .abi_encode())
    }

    pub(crate) fn bind(&self, external: &External, amount: i64) -> Result<Field, Error> {
        ext_data_hash(self, &external.ext_data(amount))
    }

}

pub(crate) fn transact(proof: &Proof, external: &External, amount: i64) -> Result<Vec<u8>, Error> {
    let parsed = ProofCalldata::parse(&proof.bytes)?;
    Ok(Pool::transactCall {
        proof: parsed.proof,
        proofCommitments: parsed.commitments,
        proofCommitmentPok: parsed.commitment_pok,
        root: word(proof.public.root),
        nullifiers: proof.public.nullifiers.map(word),
        outCommitments: proof.public.commitments.map(word),
        ext: external.ext_data(amount),
    }
    .abi_encode())
}

fn word(field: Field) -> U256 {
    U256::from_be_bytes(field.to_bytes())
}
