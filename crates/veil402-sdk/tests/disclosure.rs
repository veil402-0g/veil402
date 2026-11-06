//! Opening one payment to one auditor.
//!
//! Soundness comes from the opening hashing to the real commitment, so the
//! cases that matter are the ones where it does not: a changed amount, a
//! changed recipient, and a package shown to somebody it was not issued to.

use alloy::{
    primitives::{Address, B256, address},
    signers::local::PrivateKeySigner,
};
use veil402_sdk::{Disclosure, Field, Note, Owner, Secret};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const AUDITOR: Address = address!("00000000000000000000000000000000000000A1");

fn note() -> Result<(Note, Owner), Box<dyn std::error::Error>> {
    Ok((
        Note {
            value: 12_840_000,
            asset: Field::from(7_u64),
            random: Secret::from_bytes([3; 32])?,
        },
        Owner {
            spend: Secret::from_bytes([1; 32])?,
            view: Secret::from_bytes([2; 32])?,
        },
    ))
}

/// The whole claim: this commitment is that much, to that recipient, and the
/// auditor learns nothing else — not the change beside it, not the balance.
#[test]
fn an_opening_proves_the_payment_it_claims() -> Fallible {
    let (note, owner) = note()?;
    let issuer = PrivateKeySigner::random();
    let nonce = B256::repeat_byte(0x5E);

    let disclosure = Disclosure::issue(&note, &owner, &issuer, AUDITOR, nonce)?;

    assert_eq!(disclosure.verify(AUDITOR)?, issuer.address());
    assert_eq!(disclosure.commitment, note.commitment(&owner)?);
    assert_eq!(disclosure.value, 12_840_000);
    Ok(())
}

/// An opening that does not hash to the commitment is the one thing the
/// scheme has to refuse: claiming a payment was larger, or went elsewhere.
#[test]
fn an_opening_of_something_else_is_refused() -> Fallible {
    let (note, owner) = note()?;
    let issuer = PrivateKeySigner::random();
    let honest = Disclosure::issue(&note, &owner, &issuer, AUDITOR, B256::ZERO)?;

    for (what, tampered) in [
        ("a larger amount", Disclosure { value: 99_000_000, ..honest.clone() }),
        ("another recipient", Disclosure { owner: Field::from(9_u64), ..honest.clone() }),
        ("another asset", Disclosure { asset: Field::from(8_u64), ..honest.clone() }),
        ("a different blinding", Disclosure { random: Field::from(4_u64), ..honest.clone() }),
    ] {
        assert!(tampered.verify(AUDITOR).is_err(), "{what} verified");
    }
    Ok(())
}

/// Issued to one auditor. Passing the package on is what a viewing key
/// cannot prevent and this must.
#[test]
fn a_disclosure_does_not_travel_to_another_auditor() -> Fallible {
    let (note, owner) = note()?;
    let issuer = PrivateKeySigner::random();
    let disclosure = Disclosure::issue(&note, &owner, &issuer, AUDITOR, B256::repeat_byte(1))?;

    let somebody_else = address!("00000000000000000000000000000000000000B2");
    assert!(disclosure.verify(somebody_else).is_err());

    // Nor by rewriting who it names: the signature covers that too.
    let forwarded = Disclosure { verifier: somebody_else, ..disclosure };
    assert_ne!(
        forwarded.verify(somebody_else).ok(),
        Some(issuer.address()),
        "a disclosure was re-addressed"
    );
    Ok(())
}
