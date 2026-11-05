//! What the sync loop survives, and what stops it.

mod common;

use std::{sync::Arc, time::Duration};

use common::{Chain, Fallible, Memory};
use tokio::time::{pause, timeout};
use veil402_daemon::{
    adapters::SqliteStore,
    app::{Daemon, Halted, Retry},
    core::{index::IndexService, wallet::WalletService},
};
use veil402_sdk::Owner;

/// Built before the clock is paused: `sqlx` arms timers of its own, and
/// auto-advancing virtual time trips them.
async fn daemon(chain: Chain, attempts: u32) -> Result<Daemon, Box<dyn std::error::Error>> {
    let index = Arc::new(IndexService::load(Arc::new(chain), Arc::new(Memory::default())).await?);
    let store = Arc::new(SqliteStore::open("sqlite::memory:").await?);
    let wallet = WalletService::new(store.clone(), store, Owner::random()?);
    Ok(Daemon::new(
        index,
        Arc::new(wallet),
        Arc::new(common::Silent),
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

#[tokio::test]
async fn an_endpoint_that_stays_away_exhausts_the_loop() -> Fallible {
    let daemon = daemon(Chain::down(5), 3).await?;
    pause();

    assert!(matches!(
        daemon.run().await,
        Err(Halted::Exhausted { attempts: 3, .. })
    ));
    Ok(())
}

/// Three refusals then a pass, repeating. Counted consecutively this never
/// gives up; counted cumulatively it would stop on the fifth failure.
#[tokio::test]
async fn a_pass_that_succeeds_forgives_the_ones_before_it() -> Fallible {
    let daemon = daemon(Chain::flaky(5), 5).await?;
    pause();

    assert!(
        timeout(Duration::from_secs(600), daemon.run())
            .await
            .is_err(),
        "the loop gave up on an endpoint that keeps coming back"
    );
    Ok(())
}
