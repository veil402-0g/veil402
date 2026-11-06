//! A sealed opening has to fit what the pool will accept.
//!
//! `MAX_NOTE_BYTES` is a constant on an immutable contract, so a note that
//! outgrows it cannot be shielded at all. Measured at the largest every
//! field can be, because the encoding is fixed width and the worst case is
//! the only one worth knowing.

use veil402_sdk::{
    MAX_NOTE_BYTES, Opening, Secret,
    session::{Viewing, seal_note},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

#[test]
fn a_sealed_opening_fits_what_the_pool_accepts() -> Fallible {
    let view = Viewing::new(&Secret::from_bytes([5; 32])?).public();
    let plain = borsh::to_vec(&Opening {
        value: u64::MAX,
        asset: [0xFF; 32],
        secret: [0xFF; 32],
        session: true,
    })?;
    let sealed = seal_note(view, &plain)?;
    let wire = 32 + sealed.ciphertext.len();

    println!("opening {} bytes, on the wire {wire} of {MAX_NOTE_BYTES}", plain.len());
    assert!(
        wire <= MAX_NOTE_BYTES,
        "a sealed note is {wire} bytes, over the pool's {MAX_NOTE_BYTES}"
    );
    Ok(())
}
