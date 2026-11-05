use alloy::{
    primitives::{Address, B256, Bytes, U256, keccak256},
    sol_types::{SolCall, SolValue},
};

use super::{Pool, client::PoolClient};
use crate::{Field, client::Error};

const LOCK_TYPE: &[u8] = b"Lock(uint256 nullifier,address seller,uint64 deadline)";
const SETTLEMENT_TYPE: &[u8] =
    b"Settlement(uint256 nullifier,uint256 amount,bytes32 callsRoot)";
const DOMAIN_TYPE: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";

/// A reservation the enclave has agreed to meter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Session {
    /// Nullifier of the note held for the session.
    pub nullifier: Field,
    /// Who the enclave may pay, fixed when the note was reserved.
    pub seller: Address,
    /// Last second the seller may settle.
    pub deadline: u64,
}

impl PoolClient {
    /// Returns the pool's EIP-712 domain separator.
    #[must_use]
    pub fn domain_separator(&self) -> B256 {
        keccak256(
            (
                keccak256(DOMAIN_TYPE),
                keccak256("Veil402Pool"),
                keccak256("1"),
                U256::from(self.chain_id()),
                self.address(),
            )
                .abi_encode(),
        )
    }

    fn typed_data(&self, struct_hash: B256) -> B256 {
        let mut message = [0_u8; 66];
        message[0] = 0x19;
        message[1] = 0x01;
        message[2..34].copy_from_slice(self.domain_separator().as_slice());
        message[34..].copy_from_slice(struct_hash.as_slice());
        keccak256(message)
    }

    /// The digest an enclave signs to agree to meter `session`.
    #[must_use]
    pub fn lock_digest(&self, session: &Session) -> B256 {
        self.typed_data(keccak256(
            (
                keccak256(LOCK_TYPE),
                word(session.nullifier),
                session.seller,
                session.deadline,
            )
                .abi_encode(),
        ))
    }

    /// The digest an enclave signs to release `amount` to the seller.
    ///
    /// Signed per checkpoint over the running total, so the seller holds one
    /// signature that already covers every call so far.
    #[must_use]
    pub fn settlement_digest(&self, session: &Session, amount: u64, calls_root: B256) -> B256 {
        self.typed_data(keccak256(
            (
                keccak256(SETTLEMENT_TYPE),
                word(session.nullifier),
                U256::from(amount),
                calls_root,
            )
                .abi_encode(),
        ))
    }

    /// Encodes the reservation itself. The enclave only signs; anyone may send
    /// this.
    ///
    /// Deadline and seller are left to the pool, which has to check them
    /// anyway and is the only side that knows the current block time.
    #[must_use]
    pub fn lock(&self, session: &Session, enclave: Address, signature: &[u8]) -> Vec<u8> {
        Pool::lockCall {
            nullifier: word(session.nullifier),
            enclave,
            seller: session.seller,
            deadline: session.deadline,
            signature: Bytes::copy_from_slice(signature),
        }
        .abi_encode()
    }
}

fn word(field: Field) -> U256 {
    U256::from_be_bytes(field.to_bytes())
}

/// An enclave authorisation for one checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Voucher {
    /// Session the running total belongs to.
    pub session: Session,
    /// Cumulative amount owed, not the delta since the last checkpoint.
    pub amount: u64,
    /// Root of the per-call attestations behind this total.
    pub calls_root: B256,
    /// Enclave signature over [`PoolClient::settlement_digest`].
    pub signature: Vec<u8>,
}

impl Voucher {
    pub(crate) fn calldata(
        &self,
        proof: &crate::client::Proof,
        external: &super::External,
    ) -> Result<Vec<u8>, Error> {
        let parsed = super::ProofCalldata::parse(&proof.bytes)?;
        let amount = i64::try_from(self.amount)
            .map_err(|_| Error::Withdrawal("amount exceeds the signed protocol range"))?;
        Ok(Pool::settleToAddressCall {
            proof: parsed.proof,
            proofCommitments: parsed.commitments,
            proofCommitmentPok: parsed.commitment_pok,
            root: word(proof.public.root),
            nullifiers: proof.public.nullifiers.map(word),
            outCommitments: proof.public.commitments.map(word),
            ext: external.ext_data(-amount),
            callsRoot: self.calls_root,
            signature: Bytes::copy_from_slice(&self.signature),
        }
        .abi_encode())
    }
}
