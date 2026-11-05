use alloy::primitives::Address;

use crate::{
    Field,
    client::Error,
    protocol::note::{ASSET_DOMAIN, poseidon},
};

/// Derives the pool's asset identifier from its token address.
///
/// The pool computes the same value in its constructor, so a note proved
/// against one token can never be spent in a pool holding another.
///
/// # Errors
///
/// Returns [`Error::Poseidon`] if the hash rejects its inputs.
pub fn asset_id(token: Address) -> Result<Field, Error> {
    let mut padded = [0_u8; 32];
    padded[12..].copy_from_slice(token.as_slice());
    poseidon(&[&ASSET_DOMAIN, &Field::from_bytes(padded)?])
}
