use alloy::{
    primitives::{B256, Signature},
    signers::{SignerSync, local::PrivateKeySigner},
};
use veil402_enclave::{Call, Error, Meter, sign_lock};
use veil402_sdk::{
    Note, Secret, SessionKeys, Tree,
    evm::{PoolClient, Session, Voucher},
    session::{Handover, Keyring, Recipient, RootSeed},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn pool() -> PoolClient {
    PoolClient::new(
        "0x42532488f967329e2B090dCf68396Ef0dd63de69".parse().expect("pool"),
        "0x0000000000000000000000000000000000000abc".parse().expect("token"),
        "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC".parse().expect("verifier"),
        296,
    )
    .expect("client")
}

fn seller() -> alloy::primitives::Address {
    "0x000000000000000000000000000000000000dEaD".parse().expect("seller")
}

const DEADLINE: u64 = 1_900_000_000;

/// Fixed so a test can reconstruct the change exactly as a buyer would.
const CHANGE_SEED: [u8; 32] = [3; 32];

/// A meter over a delegation that is actually true: the note sits in the tree
/// at the leaf, under the owner the seed derives, so `open` has something real
/// to check and the ceiling is the note's own value.
fn metered(value: u64) -> Result<Meter, Box<dyn std::error::Error>> {
    let client = pool();
    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let note = Note {
        value,
        asset: client.asset(),
        random: SessionKeys::from_seed(&seed)?.random,
    };

    let mut tree = Tree::new();
    let leaf = tree.insert(note.commitment(&keys.owner)?)?;
    let witness = tree.witness(&[leaf])?;

    let session = Session {
        nullifier: keys.owner.nullifier(leaf)?,
        seller: seller(),
        deadline: DEADLINE,
    };
    let handover = Handover {
        seed,
        value,
        asset: client.asset(),
        leaf,
        siblings: witness.paths[0].siblings,
        root: witness.root,
        change_seed: Secret::from_bytes(CHANGE_SEED)?,
        seller: seller(),
        deadline: DEADLINE,
        pool: client.address(),
        chain_id: client.chain_id(),
        node: alloy::primitives::Address::ZERO,
        tee_signer: alloy::primitives::Address::ZERO,
    };

    let key = Keyring::new(RootSeed::random()?).handover(session.nullifier);
    let vouching = PrivateKeySigner::random();
    let signature = vouching.sign_hash_sync(&key.announcement(&client))?.as_bytes();
    let sealed = Recipient::accept(key.public(), &client, vouching.address(), &signature)?
        .seal(&handover.to_bytes()?)?;

    Ok(Meter::open(client, &key, &sealed, session)?)
}

fn call(cost: u64) -> Call {
    Call {
        cost,
        attestation: B256::repeat_byte(u8::try_from(cost % 256).unwrap_or(0)),
    }
}

#[test]
fn totals_accumulate_and_stop_at_the_ceiling() -> Fallible {
    let mut meter = metered(100)?;
    assert_eq!(meter.ceiling(), 100, "the note's value is the ceiling");
    assert_eq!(meter.record(call(30)).expect("first"), 30);
    assert_eq!(meter.record(call(70)).expect("second"), 100);
    assert_eq!(meter.calls(), 2);

    // Serving past the ceiling would produce a settlement nobody can prove.
    assert!(matches!(meter.record(call(1)), Err(Error::CeilingReached)));
    assert_eq!(meter.total(), 100);
    assert_eq!(meter.calls(), 2);
    Ok(())
}

#[test]
fn vouchers_are_cumulative_so_only_the_newest_matters() -> Fallible {
    let signer = PrivateKeySigner::random();
    let mut meter = metered(1_000)?;

    assert!(matches!(meter.voucher(&signer), Err(Error::NothingOwed)));

    meter.record(call(30)).expect("first");
    let first = meter.voucher(&signer).expect("voucher");
    meter.record(call(45)).expect("second");
    let second = meter.voucher(&signer).expect("voucher");

    assert_eq!(first.amount, 30);
    assert_eq!(second.amount, 75, "a voucher carries the running total");
    assert_ne!(first.calls_root, second.calls_root, "the root covers every call");
    assert_eq!(second.signature.len(), 65);
    Ok(())
}

#[test]
fn signatures_recover_to_the_enclave() -> Fallible {
    let signer = PrivateKeySigner::random();
    let client = pool();
    let mut meter = metered(1_000)?;
    let session = *meter.session();

    let lock = sign_lock(&client, &session, &signer)?;
    assert_eq!(
        Signature::try_from(lock.as_slice())?
            .recover_address_from_prehash(&client.lock_digest(&session))?,
        signer.address()
    );

    meter.record(call(30))?;
    let voucher: Voucher = meter.voucher(&signer)?;
    let digest = pool().settlement_digest(&voucher.session, voucher.amount, voucher.calls_root);
    assert_eq!(
        Signature::try_from(voucher.signature.as_slice())?.recover_address_from_prehash(&digest)?,
        signer.address()
    );
    Ok(())
}

/// The point of settling inside the enclave: a buyer that was not there
/// still finds what it is owed. The note derives from the seed it supplied
/// and the amount the chain publishes, so there is nothing to decrypt — and
/// it comes back whole, since carving a fixed size off it would only mint a
/// note nobody asked for and leave the rest somewhere else.
#[test]
fn a_settlement_leaves_a_note_the_buyer_can_reconstruct() -> Fallible {
    let mut meter = metered(1_000)?;
    meter.record(call(250))?;

    let outputs = meter.outputs()?;
    assert_eq!(outputs.len(), 1, "the change was divided");
    assert_eq!(outputs[0].value(), 750, "the whole remainder");

    // Exactly what a buyer derives knowing only its seed and the public
    // amount, and seeded — so the next envelope opens on it with no reseed.
    let next = SessionKeys::from_seed(&Secret::from_bytes(CHANGE_SEED)?)?;
    assert_eq!(outputs[0].owner(), next.owner.public()?);
    Ok(())
}

/// A session that spent its whole note leaves nothing behind.
#[test]
fn a_fully_spent_note_leaves_no_change() -> Fallible {
    let mut meter = metered(100)?;
    meter.record(call(100))?;

    assert!(meter.outputs()?.is_empty());
    Ok(())
}
