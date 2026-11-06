//! A restarted enclave picks its sessions back up, and the buyer never knows.

use std::{path::Path, sync::Arc};

use alloy::primitives::Address;
use veil402_gateway::{
    app::{Config, assemble},
    core::{MeteringService, Serving},
};
use veil402_sdk::{
    Field, Note, Secret, SessionKeys, Tree,
    evm::{PoolClient, Session},
    session::{Handover, Recipient},
    wire::Offer,
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const ROOT: [u8; 32] = [7; 32];
const VALUE: u64 = 1_000;

fn pool() -> Result<PoolClient, veil402_sdk::Error> {
    PoolClient::new(
        Address::repeat_byte(0x4A),
        Address::repeat_byte(0x7B),
        Address::repeat_byte(0x9C),
        296,
    )
}

/// A gateway over `state`. Building a second one on the same directory is a
/// restart: new meters, nothing in memory, the same records on disk.
fn gateway(state: &Path) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    Ok(assemble(&Config {
        pool: pool()?,
        root: ROOT,
        worker: "/nonexistent/worker".into(),
        artifacts: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../prover/veil402-gnark/artifacts/transaction-v4"),
        offers: vec![Offer {
            model: "stub".to_owned(),
            seller: Address::repeat_byte(0x5E),
            rate: 1,
        }],
        state: state.join("sessions.redb"),
        bind: "127.0.0.1:0".parse()?,
    })?)
}

/// Hands a real delegation over, against a note that is really in a tree.
async fn delegate(service: &MeteringService) -> Result<Field, Box<dyn std::error::Error>> {
    let client = pool()?;
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
        seller: Address::repeat_byte(0x5E),
        deadline: 1_900_000_000,
    };

    let agreed = service.agree(&session)?;
    let handover = Handover {
        seed,
        value: VALUE,
        asset: client.asset(),
        leaf,
        siblings: witness.paths[0].siblings,
        root: witness.root,
        change_owner: Field::from(42_u64),
        change_seed: Secret::random()?,
        target: 100,
        seller: session.seller,
        deadline: session.deadline,
        pool: client.address(),
        chain_id: client.chain_id(),
        node: Address::ZERO,
        tee_signer: Address::ZERO,
    };
    let key: [u8; 32] = veil402_sdk::wire::bytes(&agreed.handover_key)?
        .try_into()
        .map_err(|_| "key")?;
    let sealed = Recipient::accept(
        key,
        &client,
        agreed.signer,
        &veil402_sdk::wire::bytes(&agreed.announcement)?,
    )?
    .seal(&handover.to_bytes()?)?;

    service.accept(&sealed, session).await?;
    Ok(session.nullifier)
}

fn total(serving: &Serving) -> u64 {
    match serving {
        Serving::Metered(metered) => metered.total,
        Serving::Owing(_) => panic!("the gateway asked for payment mid-session"),
    }
}

/// The restart the buyer must not notice: a new enclave, the same seed, and
/// the total carries on from where the old one left it.
#[tokio::test]
async fn a_restarted_gateway_keeps_counting() -> Fallible {
    let state = tempfile::tempdir()?;
    let nullifier;

    {
        let service = gateway(state.path())?;
        nullifier = delegate(&service).await?;
        assert_eq!(total(&service.meter(nullifier, 40, "a").await?), 40);
        assert_eq!(total(&service.meter(nullifier, 110, "b").await?), 150);
    }

    // Everything in memory is gone. Only what the host kept remains.
    let restarted = gateway(state.path())?;
    let after = restarted.meter(nullifier, 50, "c").await?;

    assert_eq!(total(&after), 200, "the restart did not reset the meter");
    Ok(())
}

/// A session the host never saw is not a restart — it is a buyer that has
/// not paid, and the answer is the price.
#[tokio::test]
async fn an_unknown_session_is_still_asked_to_pay() -> Fallible {
    let state = tempfile::tempdir()?;
    let service = gateway(state.path())?;

    assert!(matches!(
        service.meter(Field::from(9_u64), 10, "a").await?,
        Serving::Owing(_)
    ));
    Ok(())
}


/// A retry that straddles a restart must be answered, not billed again. The
/// host's own dedupe is lost with the process, so the enclave's records carry
/// what named each call.
#[tokio::test]
async fn a_retry_across_a_restart_is_not_billed_twice() -> Fallible {
    let state = tempfile::tempdir()?;
    let nullifier;

    {
        let service = gateway(state.path())?;
        nullifier = delegate(&service).await?;
        assert_eq!(total(&service.meter(nullifier, 40, "req-1").await?), 40);
        assert_eq!(total(&service.meter(nullifier, 110, "req-2").await?), 150);
    }

    let restarted = gateway(state.path())?;
    let again = restarted.meter(nullifier, 110, "req-2").await?;

    assert_eq!(total(&again), 150, "the retry was answered, not metered");
    assert_eq!(
        total(&restarted.meter(nullifier, 50, "req-3").await?),
        200,
        "and a genuinely new call still counts"
    );
    Ok(())
}

/// A call the host never recorded — it crashed between metering and writing —
/// is counted when the client retries it, so the seller loses nothing it
/// served.
#[tokio::test]
async fn a_call_the_host_never_recorded_is_counted_on_retry() -> Fallible {
    let state = tempfile::tempdir()?;
    let nullifier;

    {
        let service = gateway(state.path())?;
        nullifier = delegate(&service).await?;
        assert_eq!(total(&service.meter(nullifier, 40, "req-1").await?), 40);
    }

    // The restart stands in for the crash: whatever was in memory and not on
    // disk is gone, and the client retries what it never got an answer to.
    let restarted = gateway(state.path())?;
    assert_eq!(
        total(&restarted.meter(nullifier, 110, "req-2").await?),
        150,
        "the retried call was counted"
    );
    Ok(())
}
