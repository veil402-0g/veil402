//! What the sync loop survives, and what stops it.

mod common;

use std::{sync::Arc, time::Duration};

use common::{Chain, Fallible, Memory};
use tokio::time::timeout;
use veil402_daemon::{
    app::{Daemon, Halted, Retry},
    core::index::IndexService,
};

async fn daemon(chain: Chain, attempts: u32) -> Result<Daemon, Box<dyn std::error::Error>> {
    let index = IndexService::load(Arc::new(chain), Arc::new(Memory::default())).await?;
    Ok(Daemon::new(
        Arc::new(index),
        Retry {
            interval: Duration::from_secs(5),
            attempts,
        },
    ))
}

/// An index that is wrong keeps serving paths until something stops it, and
/// each one costs a proof and the gas to have it rejected.
#[tokio::test]
async fn a_divergence_stops_the_loop_at_once() -> Fallible {
    let daemon = daemon(Chain::corrupting(5, 2), 8).await?;

    assert!(matches!(daemon.run().await, Err(Halted::Untrusted(_))));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_endpoint_that_stays_away_exhausts_the_loop() -> Fallible {
    let daemon = daemon(Chain::down(5), 3).await?;

    assert!(matches!(
        daemon.run().await,
        Err(Halted::Exhausted { attempts: 3, .. })
    ));
    Ok(())
}

/// Three refusals then a pass, repeating. Counted consecutively this never
/// gives up; counted cumulatively it would stop on the fifth failure.
#[tokio::test(start_paused = true)]
async fn a_pass_that_succeeds_forgives_the_ones_before_it() -> Fallible {
    let daemon = daemon(Chain::flaky(5), 5).await?;

    assert!(
        timeout(Duration::from_secs(600), daemon.run())
            .await
            .is_err(),
        "the loop gave up on an endpoint that keeps coming back"
    );
    Ok(())
}
