use std::error::Error as StdError;

use veil402_sdk::{Field, Note, Owner, Secret, SessionKeys};

/// The property delegation rests on: thirty-two bytes rebuild the note and the
/// nullifier it will reveal, and nothing else the wallet holds.
#[test]
fn a_seed_alone_reopens_its_note() -> Result<(), Box<dyn StdError>> {
    let seed = Secret::random()?;
    let asset = Field::from(7_u64);

    let minted = SessionKeys::from_seed(&seed)?;
    let note = Note {
        value: 500,
        asset,
        random: minted.random,
    };
    let commitment = note.commitment(&minted.owner)?;
    let nullifier = minted.owner.nullifier(3)?;

    // A delegate given only the seed, the value and the leaf.
    let opened = SessionKeys::from_seed(&seed)?;
    let rebuilt = Note {
        value: 500,
        asset,
        random: opened.random,
    };
    assert_eq!(rebuilt.commitment(&opened.owner)?, commitment);
    assert_eq!(opened.owner.nullifier(3)?, nullifier);
    Ok(())
}

/// Handing over one seed must not reach any other note, so the keys it opens
/// cannot be related to another seed's.
#[test]
fn seeds_are_independent_of_each_other() -> Result<(), Box<dyn StdError>> {
    let one = SessionKeys::from_seed(&Secret::random()?)?;
    let other = SessionKeys::from_seed(&Secret::random()?)?;

    assert_ne!(one.owner.public()?, other.owner.public()?);
    assert_ne!(one.owner.nullifier(0)?, other.owner.nullifier(0)?);
    Ok(())
}

/// And a session owner is not this wallet's owner, which is the whole point.
#[test]
fn a_session_owner_is_not_the_wallet_owner() -> Result<(), Box<dyn StdError>> {
    let wallet = Owner::random()?;
    let session = SessionKeys::from_seed(&Secret::random()?)?;

    assert_ne!(wallet.public()?, session.owner.public()?);
    assert_ne!(wallet.nullifier(0)?, session.owner.nullifier(0)?);
    Ok(())
}
