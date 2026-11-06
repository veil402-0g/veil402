//! A restarted enclave picks its sessions back up, and the buyer never knows.

mod support;

use std::{path::Path, sync::Arc};

use alloy::primitives::Address;
use futures_util::StreamExt;
use veil402_gateway::{
    app::{Config, assemble},
    core::{Billed, MeterError, MeteringService, Serving},
};
use veil402_sdk::{Field, session::Sealed};

use support::{DEADLINE, ROOT, asking, backing, delegate, pool};

type Fallible = Result<(), Box<dyn std::error::Error>>;


/// A gateway over `state`. Building a second one on the same directory is a
/// restart: new meters, nothing in memory, the same records on disk.
fn gateway(
    state: &Path,
    pool_state: Arc<support::Pool>,
) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    Ok(assemble(
        &Config {
            fee_cap: 20_000_000_000_000,
            pool: pool()?,
            root: ROOT,
            worker: "/nonexistent/worker".into(),
            artifacts: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../prover/veil402-gnark/artifacts/transaction-v4"),
            seller: Address::repeat_byte(0x5E),
            state: state.join("sessions.redb"),
            bind: "127.0.0.1:0".parse()?,
            rpc: "http://127.0.0.1:0".to_owned(),
            upstream: "http://127.0.0.1:0".to_owned(),
            upstream_key: "sk-test".to_owned(),
            trust: "private".to_owned(),
            scale: 1,
            margin_bps: 0,
            ceiling: 1_024,
            signer: "0x0101010101010101010101010101010101010101010101010101010101010101".to_owned(),
            multicall: alloy::primitives::Address::repeat_byte(0xCA),
            wallet_state: std::path::PathBuf::from("/nonexistent/wallet.redb"),
            confirmations: 0,
        },
        pool_state,
        Arc::new(support::Upstream { frames: 2, breaks: false, ..support::Upstream::default() }),
    )?)
}

/// Meters one call the way the route does: drain the stream, then file what
/// it cost. Without the filing a restart has nothing to resume from.
async fn metered(
    service: &Arc<MeteringService>,
    nullifier: Field,
    sealed: &Sealed,
    attempt: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let Serving::Streaming(opened) = service.meter(nullifier, sealed, attempt).await? else {
        panic!("the gateway asked for payment mid-session");
    };
    let mut frames = opened.frames;
    let mut total = None;
    while let Some(billed) = frames.next().await {
        if let Billed::Bill(metering) = billed? {
            service.billed(nullifier, attempt, sealed, &metering).await?;
            total = Some(metering.metered.total);
        }
    }
    Ok(total.expect("a call that ended without a bill"))
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
        first = metered(&service, nullifier, &asking(&sealing, 40)?, "a").await?;
        second = metered(&service, nullifier, &asking(&sealing, 110)?, "b").await?;
        assert!(second > first, "the second call added to the first");
    }

    // Everything in memory is gone. Only what the host kept remains.
    let restarted = gateway(state.path(), held)?;
    let third = metered(&restarted, nullifier, &asking(&sealing, 50)?, "c").await?;

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

/// A retry that straddles a restart must be refused, not served again.
///
/// It used to be answered for free, which cost nothing when the answer came
/// from a stub. It now comes from a provider that charges for it, so the
/// only safe reply is no — and the enclave's own records are what carry the
/// knowledge that this call already happened.
#[tokio::test]
async fn a_retry_across_a_restart_is_refused_rather_than_bought_again() -> Fallible {
    let state = tempfile::tempdir()?;
    let held = backing(DEADLINE)?;
    let (nullifier, sealing, second);

    {
        let service = gateway(state.path(), held.clone())?;
        let delegated = delegate(&service).await?;
        nullifier = delegated.nullifier;
        sealing = delegated.sealing;
        metered(&service, nullifier, &asking(&sealing, 40)?, "req-1").await?;
        second = metered(&service, nullifier, &asking(&sealing, 110)?, "req-2").await?;
    }

    let restarted = gateway(state.path(), held)?;
    assert!(
        matches!(
            restarted
                .meter(nullifier, &asking(&sealing, 110)?, "req-2")
                .await,
            Err(MeterError::Repeated)
        ),
        "a call already served was bought a second time"
    );
    assert!(
        metered(&restarted, nullifier, &asking(&sealing, 50)?, "req-3").await? > second,
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
        first = metered(&service, nullifier, &asking(&sealing, 40)?, "req-1").await?;
    }

    // The restart stands in for the crash: whatever was in memory and not on
    // disk is gone, and the client retries what it never got an answer to.
    let restarted = gateway(state.path(), held)?;
    assert!(
        metered(&restarted, nullifier, &asking(&sealing, 110)?, "req-2").await? > first,
        "the retried call was counted"
    );
    Ok(())
}

/// What the sweep has to find: sessions close enough to their deadline that
/// waiting for the buyer is no longer safe, and nothing else.
#[tokio::test]
async fn only_sessions_near_their_deadline_are_due() -> Fallible {
    use veil402_gateway::{adapters::Sessions, core::Resumes};

    let state = tempfile::tempdir()?;
    let sessions = Sessions::open(&state.path().join("sessions.redb"))?;
    let sealed = veil402_sdk::session::Sealed {
        encapsulated: [0; 32],
        ciphertext: Vec::new(),
    };
    for (nullifier, deadline) in [(1_u64, 1_000_u64), (2, 5_000), (3, 1_500)] {
        sessions
            .opened(
                Field::from(nullifier),
                &sealed,
                veil402_sdk::evm::Session {
                    nullifier: Field::from(nullifier),
                    seller: Address::repeat_byte(0x5E),
                    deadline,
                },
            )
            .await?;
    }

    let mut due: Vec<u64> = sessions
        .expiring(1_500)
        .await?
        .iter()
        .map(|nullifier| u64::from_be_bytes(nullifier.to_bytes()[24..].try_into().expect("tail")))
        .collect();
    due.sort_unstable();
    assert_eq!(due, vec![1, 3], "the wrong sessions came due");

    // A settled session is nobody's business, so it stops being found.
    sessions.forget(Field::from(1_u64)).await?;
    assert_eq!(sessions.expiring(1_500).await?.len(), 1);
    Ok(())
}

/// A batch is only worth the hop when there is more than one, and a buyer
/// that settled its own session a moment ago must not take the rest down
/// with it.
#[test]
fn a_lone_settlement_skips_the_batcher_and_a_batch_tolerates_a_loser() {
    use veil402_gateway::{adapters::batched, core::Settlement};

    let pool = Address::repeat_byte(0x4A);
    let multicall = Address::repeat_byte(0xCA);
    let settling = |calldata: Vec<u8>| Settlement {
        nullifier: Field::from(1_u64),
        calldata,
    };

    assert!(batched(pool, multicall, &[]).is_none(), "an empty pass sent something");

    let one = settling(vec![1, 2, 3]);
    let (to, calldata) = batched(pool, multicall, &[&one]).expect("one");
    assert_eq!(to, pool, "a single settlement went the long way round");
    assert_eq!(calldata.as_ref(), &[1, 2, 3]);

    let two = settling(vec![4, 5, 6]);
    let (to, calldata) = batched(pool, multicall, &[&one, &two]).expect("many");
    assert_eq!(to, multicall);
    // allowFailure, twice: the encoding carries a true for every call.
    assert_eq!(
        calldata
            .windows(32)
            .filter(|word| word[..31].iter().all(|byte| *byte == 0) && word[31] == 1)
            .count(),
        2,
        "a failing settlement would take the batch with it"
    );
}
