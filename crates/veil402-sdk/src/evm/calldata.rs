use alloy::primitives::U256;
use ark_bn254::{Fq, Fq2, G1Affine, G2Affine};

use crate::{
    client::Error,
    protocol::encoding::{field_bytes, parse_field},
};

/// Raw gnark proof layout: `Ar(64) ‖ Bs(128) ‖ Krs(64) ‖ count(4) ‖ commitment(64) ‖ pok(64)`.
const RAW_PROOF_LEN: usize = 388;

/// A Groth16 proof in the argument layout the exported verifier expects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProofCalldata {
    /// `Ar`, `Bs` and `Krs` flattened, with `Bs` in EIP-197 order.
    pub proof: [U256; 8],
    /// gnark's Pedersen commitment to the private witness.
    pub commitments: [U256; 2],
    /// Proof of knowledge for that commitment.
    pub commitment_pok: [U256; 2],
}

impl ProofCalldata {
    /// Parses a raw gnark proof.
    ///
    /// Coordinates are checked against the curve rather than copied, so a
    /// malformed proof fails here instead of consuming gas on chain.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Prover`] when the length, commitment count, or any
    /// point is invalid.
    pub fn parse(raw: &[u8]) -> Result<Self, Error> {
        if raw.len() != RAW_PROOF_LEN {
            return Err(Error::Prover("proof is not 388 bytes"));
        }
        let count = u32::from_be_bytes(
            raw[256..260].try_into().map_err(|_| Error::Prover("truncated proof"))?,
        );
        if count != 1 {
            return Err(Error::Prover("expected exactly one proof commitment"));
        }

        let ar = g1(&raw[0..64])?;
        let bs = g2(&raw[64..192])?;
        let krs = g1(&raw[192..256])?;
        let commitment = g1(&raw[260..324])?;
        let pok = g1(&raw[324..388])?;

        Ok(Self {
            // EIP-197 orders the quadratic extension imaginary part first.
            proof: [
                word(ar.x),
                word(ar.y),
                word(bs.x.c1),
                word(bs.x.c0),
                word(bs.y.c1),
                word(bs.y.c0),
                word(krs.x),
                word(krs.y),
            ],
            commitments: [word(commitment.x), word(commitment.y)],
            commitment_pok: [word(pok.x), word(pok.y)],
        })
    }
}

fn word(value: Fq) -> U256 {
    U256::from_be_bytes(field_bytes(value))
}

fn coordinate(bytes: &[u8]) -> Result<Fq, Error> {
    let fixed: [u8; 32] = bytes.try_into().map_err(|_| Error::Prover("truncated proof"))?;
    parse_field(&fixed).ok_or(Error::Prover("proof coordinate is not canonical"))
}

fn g1(bytes: &[u8]) -> Result<G1Affine, Error> {
    let point = G1Affine::new_unchecked(coordinate(&bytes[0..32])?, coordinate(&bytes[32..64])?);
    if point.is_on_curve() && point.is_in_correct_subgroup_assuming_on_curve() {
        Ok(point)
    } else {
        Err(Error::Prover("proof point is not on the curve"))
    }
}

fn g2(bytes: &[u8]) -> Result<G2Affine, Error> {
    // gnark serialises the imaginary part first in each coordinate.
    let x = Fq2::new(coordinate(&bytes[32..64])?, coordinate(&bytes[0..32])?);
    let y = Fq2::new(coordinate(&bytes[96..128])?, coordinate(&bytes[64..96])?);
    let point = G2Affine::new_unchecked(x, y);
    if point.is_on_curve() && point.is_in_correct_subgroup_assuming_on_curve() {
        Ok(point)
    } else {
        Err(Error::Prover("proof point is not on the curve"))
    }
}
