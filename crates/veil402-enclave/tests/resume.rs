//! Picking a session back up from records a host kept but cannot read.

use alloy::{primitives::B256, signers::local::PrivateKeySigner};
use veil402_enclave::{Call, Error, Meter, record, replay};
use veil402_sdk::{
    Note, Secret, SessionKeys, Tree,
    evm::{PoolClient, Session},
    session::{EnclaveKey, Handover, Keyring, Recipient, RootSeed, Sealed},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const DEADLINE: u64 = 1_900_000_000;
const VALUE: u64 = 1_000;

fn pool() -> PoolClient {
    PoolClient::new(
        "0x42532488f967329e2B090dCf68396Ef0dd63de69".parse().expect("pool"),
        "0x0000000000000000000000000000000000000abc".parse().expect("token"),
        "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC".parse().expect("verifier"),
        296,
    )
    .expect("client")
}

struct Delegation {
    sealed: Sealed,
    session: Session,
    key: EnclaveKey,
}

/// A delegation this enclave will accept, against a note that is really in a
/// tree at the leaf it names.
fn delegated(keyring: &Keyring) -> Result<Delegation, Box<dyn std::error::Error>> {
    let client = pool();
    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let note = Note {
        value: VALUE,
        asset: client.asset(),
        random: SessionKeys::from_seed(&seed)?.random,
    };
    let mut tree = Tree::new();
    let leaf = tree.insert(note.commitment(&keys.owner)?)?;
    let witness = tree.witness(&[leaf])?;
    let session = Session {
        nullifier: keys.owner.nullifier(leaf)?,
        seller: "0x000000000000000000000000000000000000dEaD".parse()?,
        deadline: DEADLINE,
    };
    let handover = Handover {
        seed,
        value: VALUE,
        asset: client.asset(),
        leaf,
        siblings: witness.paths[0].siblings,
        root: witness.root,
        change_seed: Secret::random()?,
        view: [0; 32],
        seller: session.seller,
        deadline: DEADLINE,
        pool: client.address(),
        chain_id: client.chain_id(),
        node: alloy::primitives::Address::ZERO,
        tee_signer: alloy::primitives::Address::ZERO,
    };
    let key = keyring.handover(session.nullifier);
    let vouching = PrivateKeySigner::random();
    let signature = alloy::signers::SignerSync::sign_hash_sync(
        &vouching,
        &key.announcement(&client),
    )?
    .as_bytes();
    let sealed = Recipient::accept(key.public(), &client, vouching.address(), &signature)?
        .seal(&handover.to_bytes()?)?;
    Ok(Delegation {
        sealed,
        session,
        key,
    })
}

fn call(cost: u64) -> Call {
    Call {
        cost,
        attestation: B256::repeat_byte(u8::try_from(cost % 256).unwrap_or(0)),
    }
}

/// The enclave holds nothing durable, so a restart has to come back to the
/// same total — or the seller loses everything served before it.
#[test]
fn a_restart_resumes_the_total_it_sealed() -> Fallible {
    let keyring = Keyring::new(RootSeed::from_bytes([7; 32]));
    let delegation = delegated(&keyring)?;

    let mut meter = Meter::open(
        pool(),
        &delegation.key,
        &delegation.sealed,
        delegation.session,
    )?;
    let mut log = Vec::new();
    for (sequence, cost) in [40_u64, 110].into_iter().enumerate() {
        let served = call(cost);
        let total = meter.record(served)?;
        log.push(record(
            &delegation.key,
            sequence as u64,
            &format!("req-{sequence}"),
            total,
            served.attestation,
        )?);
    }
    let before = meter.calls_root();
    drop(meter);

    // A new process, the same root seed, and nothing but what the host kept.
    let revived = Keyring::new(RootSeed::from_bytes([7; 32]));
    let (mut meter, recorded) = Meter::resume(
        pool(),
        &revived.handover(delegation.session.nullifier),
        &delegation.sealed,
        delegation.session,
        &log,
    )?;

    assert_eq!(meter.total(), 150, "the total survived the restart");
    assert_eq!(meter.calls(), 2);
    assert_eq!(meter.calls_root(), before, "a buyer can still prove its calls");
    assert_eq!(meter.record(call(50))?, 200, "and it keeps counting");
    assert_eq!(
        recorded.iter().map(|r| r.attempt.as_str()).collect::<Vec<_>>(),
        vec!["req-0", "req-1"],
        "what was already answered comes back, so a retry is not billed twice"
    );
    Ok(())
}

/// The host is not trusted with the total. A log it edited must not open, or
/// it could bill a buyer for work nobody did.
#[test]
fn a_log_the_enclave_did_not_seal_is_refused() -> Fallible {
    let keyring = Keyring::new(RootSeed::from_bytes([7; 32]));
    let delegation = delegated(&keyring)?;
    let mut sealed = record(&delegation.key, 0, "req-0", 150, B256::repeat_byte(1))?;

    assert!(replay(&delegation.key, &[sealed.clone()]).is_ok());

    // One flipped byte of ciphertext.
    let last = sealed.len() - 1;
    sealed[last] ^= 1;
    assert!(matches!(
        replay(&delegation.key, &[sealed]),
        Err(Error::Veil(_))
    ));

    // A record sealed under another seed, which is what a host inventing a
    // total would have to produce.
    let forged = record(
        &Keyring::new(RootSeed::from_bytes([8; 32])).handover(delegation.session.nullifier),
        0,
        "req-0",
        9_999,
        B256::repeat_byte(1),
    )?;
    assert!(replay(&delegation.key, &[forged]).is_err());
    Ok(())
}

/// A log with a hole is a log the host has edited.
#[test]
fn a_log_that_skips_a_record_is_refused() -> Fallible {
    let keyring = Keyring::new(RootSeed::from_bytes([7; 32]));
    let delegation = delegated(&keyring)?;
    let second = record(&delegation.key, 1, "req-1", 150, B256::repeat_byte(1))?;

    assert!(matches!(
        replay(&delegation.key, &[second]),
        Err(Error::Refused(_))
    ));
    Ok(())
}
