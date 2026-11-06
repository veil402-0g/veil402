//! The channel a session's authority travels down, and the checks that stop it
//! being pointed at the wrong note.

use std::error::Error as StdError;

use alloy::{
    primitives::Address,
    signers::{SignerSync, local::PrivateKeySigner},
};
use veil402_sdk::{
    Field, Note, Secret, SessionKeys, TREE_DEPTH, Tree,
    evm::{PoolClient, Session},
    session::{EnclaveKey, Handover, Keyring, Recipient, RootSeed},
};

type Fallible = Result<(), Box<dyn StdError>>;

/// A key no other call in this file shares, which is what these tests mean by
/// "another enclave".
fn fresh() -> Result<EnclaveKey, Box<dyn StdError>> {
    Ok(Keyring::new(RootSeed::random()?).handover(Field::from(1_u64)))
}

const CHAIN: u64 = 296;
const POOL: Address = Address::new([0x11; 20]);
const SELLER: Address = Address::new([0x22; 20]);
const DEADLINE: u64 = 1_800_000_000;

fn pool() -> Result<PoolClient, Box<dyn StdError>> {
    Ok(PoolClient::new(
        POOL,
        Address::new([0x55; 20]),
        Address::new([0x66; 20]),
        CHAIN,
    )?)
}

/// A handover that should pass every check: the note is really in the tree at
/// the leaf, under the owner the seed derives.
fn valid() -> Result<(Handover, Session), Box<dyn StdError>> {
    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let asset = Field::from(7_u64);
    let note = Note {
        value: 500,
        asset,
        random: SessionKeys::from_seed(&seed)?.random,
    };

    let mut tree = Tree::new();
    tree.insert(Field::from(1_u64))?; // a stranger's note at leaf 0
    let leaf = tree.insert(note.commitment(&keys.owner)?)?;
    let witness = tree.witness(&[leaf])?;

    let session = Session {
        nullifier: keys.owner.nullifier(leaf)?,
        seller: SELLER,
        deadline: DEADLINE,
    };
    Ok((
        Handover {
            seed,
            value: 500,
            asset,
            leaf,
            siblings: witness.paths[0].siblings,
            root: witness.root,
            change_seed: Secret::random()?,
            seller: SELLER,
            deadline: DEADLINE,
            pool: POOL,
            chain_id: CHAIN,
            node: Address::new([0x33; 20]),
            tee_signer: Address::new([0x44; 20]),
        },
        session,
    ))
}

/// A buyer may only seal to a key the enclave vouched for with the signer the
/// registry knows.
#[test]
fn only_a_vouched_key_can_be_sealed_to() -> Fallible {
    let pool = pool()?;
    let signer = PrivateKeySigner::random();
    let key = fresh()?;
    let signature = signer.sign_hash_sync(&key.announcement(&pool))?.as_bytes();

    let recipient = Recipient::accept(key.public(), &pool, signer.address(), &signature)?;
    let sealed = recipient.seal(b"authority")?;
    assert_eq!(key.open(&sealed)?, b"authority");

    // Signed by someone the registry does not know.
    let stranger = PrivateKeySigner::random();
    assert!(
        Recipient::accept(key.public(), &pool, stranger.address(), &signature).is_err(),
        "accepted a key vouched for by a stranger"
    );
    Ok(())
}

/// EIP-712 puts the chain and the pool in the domain separator, so an
/// announcement cannot be lifted from one deployment to another.
#[test]
fn an_announcement_is_bound_to_its_deployment() -> Fallible {
    let key = fresh()?;
    let here = key.announcement(&pool()?);

    let other_chain = PoolClient::new(POOL, Address::ZERO, Address::ZERO, CHAIN + 1)?;
    let other_pool = PoolClient::new(
        Address::new([0x99; 20]),
        Address::ZERO,
        Address::ZERO,
        CHAIN,
    )?;
    assert_ne!(here, key.announcement(&other_chain));
    assert_ne!(here, key.announcement(&other_pool));
    assert_ne!(here, fresh()?.announcement(&pool()?));
    Ok(())
}

#[test]
fn an_envelope_opens_only_for_its_recipient() -> Fallible {
    let pool = pool()?;
    let signer = PrivateKeySigner::random();
    let key = fresh()?;
    let signature = signer.sign_hash_sync(&key.announcement(&pool))?.as_bytes();
    let sealed =
        Recipient::accept(key.public(), &pool, signer.address(), &signature)?.seal(b"authority")?;

    assert!(fresh()?.open(&sealed).is_err(), "another enclave opened it");

    let mut tampered = sealed.clone();
    tampered.ciphertext[0] ^= 1;
    assert!(key.open(&tampered).is_err(), "a flipped byte still opened");
    Ok(())
}

#[test]
fn a_handover_survives_the_wire() -> Fallible {
    let (handover, _) = valid()?;
    let bytes = handover.to_bytes()?;
    let back = Handover::from_bytes(&bytes)?;

    assert_eq!(back.value, handover.value);
    assert_eq!(back.leaf, handover.leaf);
    assert_eq!(back.root, handover.root);
    assert_eq!(back.tee_signer, handover.tee_signer);
    assert_eq!(back.to_bytes()?, bytes);
    Ok(())
}

/// A delegate and a buyer upgrade independently, so a version it does not know
/// is refused rather than half-understood.
#[test]
fn an_unknown_version_is_refused() -> Fallible {
    let (handover, _) = valid()?;
    let mut bytes = handover.to_bytes()?;
    bytes[0] = 99;
    assert!(Handover::from_bytes(&bytes).is_err());

    let good = handover.to_bytes()?;
    assert!(
        Handover::from_bytes(&good[..good.len() - 1]).is_err(),
        "short payload accepted"
    );
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(
        Handover::from_bytes(&trailing).is_err(),
        "trailing bytes accepted"
    );
    Ok(())
}

#[test]
fn a_valid_handover_is_accepted() -> Fallible {
    let (handover, session) = valid()?;
    let keys = handover.accept(&session)?;

    assert_eq!(keys.owner.nullifier(handover.leaf)?, session.nullifier);
    Ok(())
}

/// The seed is the delegation. One that opens a different note would let a
/// delegate meter a session it can never settle.
#[test]
fn a_seed_for_another_note_is_refused() -> Fallible {
    let (mut handover, session) = valid()?;
    handover.seed = Secret::random()?;

    assert!(handover.accept(&session).is_err());
    Ok(())
}

/// The commitment is built from the value and placed by the leaf; the siblings
/// carry it to the root. Each has to be the one the pool recorded.
#[test]
fn a_tampered_path_is_refused() -> Fallible {
    let (handover, session) = valid()?;
    let clone = || Handover::from_bytes(&handover.to_bytes().expect("encode"));

    let mut value = clone()?;
    value.value += 1;
    assert!(value.accept(&session).is_err(), "a changed value passed");

    let mut leaf = clone()?;
    leaf.leaf += 1;
    assert!(leaf.accept(&session).is_err(), "a changed leaf passed");

    let mut sibling = clone()?;
    sibling.siblings[TREE_DEPTH - 1] = Field::from(99_u64);
    assert!(
        sibling.accept(&session).is_err(),
        "a changed sibling passed"
    );

    let mut root = clone()?;
    root.root = Field::from(99_u64);
    assert!(root.accept(&session).is_err(), "a changed root passed");
    Ok(())
}

/// The escrow fixes who may be paid and until when; a handover disagreeing
/// with it is for some other session.
#[test]
fn a_handover_for_another_session_is_refused() -> Fallible {
    let (handover, session) = valid()?;

    let mut elsewhere = Handover::from_bytes(&handover.to_bytes()?)?;
    elsewhere.seller = Address::new([0x77; 20]);
    assert!(elsewhere.accept(&session).is_err());

    let mut later = Handover::from_bytes(&handover.to_bytes()?)?;
    later.deadline = DEADLINE + 1;
    assert!(later.accept(&session).is_err());
    Ok(())
}
