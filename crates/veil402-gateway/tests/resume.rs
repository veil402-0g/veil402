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
        },
        pool_state,
        Arc::new(support::Upstream { frames: 2, breaks: false }),
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
