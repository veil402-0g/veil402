use alloy::{
    primitives::{Address, Bytes, I256, U256, keccak256},
    sol_types::SolValue,
};

use super::{Pool::ExtData, client::PoolClient};
use crate::{Field, client::Error, protocol::{MAX_NOTE_BYTES, Plan}};

/// BN254 scalar field modulus, matching `SNARK_SCALAR_FIELD` on chain.
pub(crate) const SNARK_SCALAR_FIELD: U256 = U256::from_limbs([
    0x43E1_F593_F000_0001,
    0x2833_E848_79B9_7091,
    0xB850_45B6_8181_585D,
    0x3064_4E72_E131_A029,
]);

const BINDING_DOMAIN: &[u8] = b"veil402:transact:v5";

/// Output ciphertexts in the order the planner fixed, with the withdrawal
/// target. Built from a plan so the order can never be chosen separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct External {
    recipient: Address,
    encrypted: [Vec<u8>; 2],
}

impl External {
    pub(crate) fn new(plan: &Plan, recipient: Address) -> Result<Self, Error> {
        let encrypted = plan.encrypted();
        if encrypted.iter().any(|note| note.len() > MAX_NOTE_BYTES) {
            return Err(Error::Withdrawal("encrypted note exceeds 128 bytes"));
        }
        Ok(Self {
            recipient,
            encrypted: [encrypted[0].to_vec(), encrypted[1].to_vec()],
        })
    }

    pub(crate) fn ext_data(&self, amount: i64) -> ExtData {
        ExtData {
            recipient: self.recipient,
            extAmount: I256::unchecked_from(amount),
            notes: [
                Bytes::copy_from_slice(&self.encrypted[0]),
                Bytes::copy_from_slice(&self.encrypted[1]),
            ],
        }
    }
}

/// Derives the external-data hash the pool recomputes on chain.
///
/// Ciphertexts are length-prefixed so bytes cannot shift from one note into the
/// other. Chain, pool, token and verifier are all bound, so a proof is useless
/// anywhere but the deployment it was built for.
///
/// # Errors
///
/// Returns [`Error::Withdrawal`] when the amount is positive or a ciphertext
/// exceeds the pool's limit.
pub fn ext_data_hash(pool: &PoolClient, ext: &ExtData) -> Result<Field, Error> {
    if ext.extAmount.is_positive() {
        return Err(Error::Withdrawal("deposits must use shield"));
    }
    if ext.notes.iter().any(|note| note.len() > MAX_NOTE_BYTES) {
        return Err(Error::Withdrawal("ciphertext exceeds the pool limit"));
    }

    let mut packed = Vec::new();
    for note in &ext.notes {
        let length = u32::try_from(note.len())
            .map_err(|_| Error::Withdrawal("ciphertext exceeds the pool limit"))?;
        packed.extend_from_slice(&length.to_be_bytes());
        packed.extend_from_slice(note);
    }

    let encoded = (
        keccak256(BINDING_DOMAIN),
        U256::from(pool.chain_id()),
        pool.address(),
        pool.token(),
        pool.verifier(),
        ext.recipient,
        ext.extAmount,
        keccak256(&packed),
    )
        .abi_encode();

    let reduced = U256::from_be_bytes(keccak256(encoded).0) % SNARK_SCALAR_FIELD;
    Field::from_bytes(reduced.to_be_bytes())
}

/// Encodes a signed public amount the way the circuit reads it: a withdrawal is
/// the additive inverse in the field, not a sign bit.
#[must_use]
pub fn public_amount(amount: i64) -> Field {
    Field::from_signed(amount)
}

/// A validated public withdrawal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Withdrawal {
    recipient: Address,
    amount: i64,
}

impl Withdrawal {
    /// Validates a withdrawal destination and amount.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Withdrawal`] for a zero, unrepresentable, or burning
    /// withdrawal.
    pub fn new(recipient: Address, amount: u64) -> Result<Self, Error> {
        if amount == 0 {
            return Err(Error::Withdrawal("amount must be greater than zero"));
        }
        if recipient.is_zero() {
            return Err(Error::Withdrawal("recipient must not be the zero address"));
        }
        let amount = i64::try_from(amount)
            .map_err(|_| Error::Withdrawal("amount exceeds the signed protocol range"))?;
        Ok(Self { recipient, amount })
    }

    /// Returns the recipient.
    #[must_use]
    pub const fn recipient(&self) -> Address {
        self.recipient
    }

    /// Returns the withdrawn token amount.
    #[must_use]
    pub const fn amount(&self) -> u64 {
        self.amount.unsigned_abs()
    }

    pub(crate) const fn public_amount(self) -> i64 {
        -self.amount
    }
}
