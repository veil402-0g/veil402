//! A restarted enclave picks its sessions back up, and the buyer never knows.

mod support;

use std::{path::Path, sync::Arc};

use alloy::primitives::Address;
use veil402_gateway::{
    app::{Config, assemble},
    core::{MeteringService, Serving},
};
use veil402_sdk::{
    Field, Note, Secret, SessionKeys, Tree,
    evm::{PoolClient, Session},
    session::{Exchange, Handover, Keyring, Recipient, RootSeed, Sealed},
    wire::{Message, Offer, Prompt},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const ROOT: [u8; 32] = [7; 32];
const VALUE: u64 = 1_000;
const DEADLINE: u64 = 1_900_000_000;

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
fn gateway(
    state: &Path,
    pool_state: Arc<support::Pool>,
) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    Ok(assemble(
        &Config {
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
            rpc: "http://127.0.0.1:0".to_owned(),
        },
        pool_state,
    )?)
}

/// A pool backing the one session these tests open. The enclave's signer is
/// derived from the root, so the escrow has to name the same address.
fn backing(deadline: u64) -> Result<Arc<support::Pool>, Box<dyn std::error::Error>> {
    let signer = Keyring::new(RootSeed::from_bytes(ROOT)).signer()?.address();
    Ok(Arc::new(support::Pool::backing(
        signer,
        Address::repeat_byte(0x5E),
        deadline,
    )))
}

/// Hands a real delegation over, against a note that is really in a tree.
async fn delegate(service: &MeteringService) -> Result<Delegated, Box<dyn std::error::Error>> {
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
        deadline: DEADLINE,
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
    Ok(Delegated {
        nullifier: session.nullifier,
        sealing: Recipient::accept(
            key,
            &client,
            agreed.signer,
            &veil402_sdk::wire::bytes(&agreed.announcement)?,
        )?,
    })
}

/// What a daemon keeps after handing a delegation over: which envelope, and
/// the key its requests seal to.
struct Delegated {
    nullifier: Field,
    sealing: Recipient,
}

/// A request of a given size, sealed the way a daemon seals one.
fn asking(sealing: &Recipient, size: usize) -> Result<Sealed, Box<dyn std::error::Error>> {
    let prompt = Prompt {
        model: "stub".to_owned(),
        messages: vec![Message {
            role: "user".to_owned(),
            content: "x".repeat(size),
        }],
        stream: false,
        reply_to: Exchange::open().public(),
    };
    Ok(sealing.seal_prompt(&borsh::to_vec(&prompt)?)?)
}

fn total(serving: &Serving) -> u64 {
    match serving {
        Serving::Metered(served) => served.metered.total,
        Serving::Owing(_) => panic!("the gateway asked for payment mid-session"),
    }
}

/// The restart the buyer must not notice: a new enclave, the same seed, and
/// the total carries on from where the old one left it.
#[tokio::test]
async fn a_restarted_gateway_keeps_counting() -> Fallible {
    let state = tempfile::tempdir()?;
    let held = backing(DEADLINE)?;
    let (nullifier, sealing, first, second);

    {
        let service = gateway(state.path(), held.clone())?;
        let delegated = delegate(&service).await?;
        nullifier = delegated.nullifier;
        sealing = delegated.sealing;
        first = total(&service.meter(nullifier, &asking(&sealing, 40)?, "a").await?);
        second = total(&service.meter(nullifier, &asking(&sealing, 110)?, "b").await?);
        assert!(second > first, "the second call added to the first");
    }

    // Everything in memory is gone. Only what the host kept remains.
    let restarted = gateway(state.path(), held)?;
    let third = total(&restarted.meter(nullifier, &asking(&sealing, 50)?, "c").await?);

    assert!(
        third > second,
        "the restart reset the meter: {third} should exceed {second}"
    );
    Ok(())
}

/// A session the host never saw is not a restart — it is a buyer that has
/// not paid, and the answer is the price.
#[tokio::test]
async fn an_unknown_session_is_still_asked_to_pay() -> Fallible {
    let state = tempfile::tempdir()?;
    let service = gateway(state.path(), backing(DEADLINE)?)?;
    let sealing = delegate(&service).await?.sealing;

    assert!(matches!(
        service
            .meter(Field::from(9_u64), &asking(&sealing, 10)?, "a")
            .await?,
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
    let held = backing(DEADLINE)?;
    let (nullifier, sealing, second);

    {
        let service = gateway(state.path(), held.clone())?;
        let delegated = delegate(&service).await?;
        nullifier = delegated.nullifier;
        sealing = delegated.sealing;
        total(&service.meter(nullifier, &asking(&sealing, 40)?, "req-1").await?);
        second = total(&service.meter(nullifier, &asking(&sealing, 110)?, "req-2").await?);
    }

    let restarted = gateway(state.path(), held)?;
    let again = total(&restarted.meter(nullifier, &asking(&sealing, 110)?, "req-2").await?);

    assert_eq!(again, second, "the retry was answered, not metered again");
    assert!(
        total(&restarted.meter(nullifier, &asking(&sealing, 50)?, "req-3").await?) > second,
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
    let held = backing(DEADLINE)?;
    let (nullifier, sealing, first);

    {
        let service = gateway(state.path(), held.clone())?;
        let delegated = delegate(&service).await?;
        nullifier = delegated.nullifier;
        sealing = delegated.sealing;
        first = total(&service.meter(nullifier, &asking(&sealing, 40)?, "req-1").await?);
    }

    // The restart stands in for the crash: whatever was in memory and not on
    // disk is gone, and the client retries what it never got an answer to.
    let restarted = gateway(state.path(), held)?;
    assert!(
        total(&restarted.meter(nullifier, &asking(&sealing, 110)?, "req-2").await?) > first,
        "the retried call was counted"
    );
    Ok(())
}
