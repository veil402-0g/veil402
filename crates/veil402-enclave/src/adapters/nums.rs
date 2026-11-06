//! The peer point the seed is agreed against.
//!
//! The whole arrangement rests on nobody knowing this point's discrete log —
//! including us. So it is not generated from a scalar: it is hashed onto the
//! curve from a fixed string by RFC 9380, which lands on a group element
//! indistinguishable from a random one.
//!
//! Both inputs below are load-bearing and may never change. Together they fix
//! the point; the point fixes the shared secret; the shared secret fixes every
//! key the enclave holds. Changing either silently rotates the seller's
//! identity, and the commitment gate is what turns that into a refusal.

use elliptic_curve::{
    hash2curve::{ExpandMsgXmd, GroupDigest},
    sec1::ToEncodedPoint,
};
use p384::{NistP384, PublicKey};
use sha2::Sha384;
use spki::EncodePublicKey;

const NUMS_MSG: &[u8] = b"veil402/nums-peer/v1";
const NUMS_DST: &[u8] = b"VEIL402-NUMS-P384_XMD:SHA-384_SSWU_RO_";

#[derive(Debug, thiserror::Error)]
#[error("the peer point will not derive: {0}")]
pub struct NumsError(String);

/// Derives the point from the compiled strings.
///
/// Recomputed rather than pinned as a byte array: the strings are what is
/// measured into the image either way, and a constant beside them would be a
/// second thing to keep true.
///
/// # Errors
///
/// Returns [`NumsError`] when the curve rejects the result, which takes a
/// break in the hash rather than bad luck.
pub fn peer() -> Result<PublicKey, NumsError> {
    let point = NistP384::hash_from_bytes::<ExpandMsgXmd<Sha384>>(&[NUMS_MSG], &[NUMS_DST])
        .map_err(|error| NumsError(error.to_string()))?;
    // Rejects the identity, which has a known discrete log and would hand the
    // same secret to anyone who asked.
    PublicKey::from_affine(point.to_affine()).map_err(|error| NumsError(error.to_string()))
}

/// The point as KMS wants it: `SubjectPublicKeyInfo` DER.
///
/// # Errors
///
/// Returns [`NumsError`] when the point will not encode.
pub fn peer_spki() -> Result<Vec<u8>, NumsError> {
    Ok(peer()?
        .to_public_key_der()
        .map_err(|error| NumsError(error.to_string()))?
        .as_bytes()
        .to_vec())
}

/// The compressed point, bound into the seed's KDF so the seed is tied to the
/// peer it was agreed with.
///
/// # Errors
///
/// Returns [`NumsError`] when the point will not derive.
pub fn peer_compressed() -> Result<Vec<u8>, NumsError> {
    Ok(peer()?.to_encoded_point(true).as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spki::DecodePublicKey;

    /// Two derivations must agree, or the seed moves when nothing did.
    #[test]
    fn the_point_is_stable() {
        assert_eq!(peer().expect("derives"), peer().expect("derives"));
    }

    /// A point that does not round-trip through SPKI is one KMS would refuse.
    #[test]
    fn the_point_encodes_as_kms_expects() {
        let der = peer_spki().expect("encodes");
        assert_eq!(
            PublicKey::from_public_key_der(&der).expect("decodes"),
            peer().expect("derives")
        );
    }
}
