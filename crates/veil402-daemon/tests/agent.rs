//! The ledger: who may spend, what they have spent, and what stops them.
//!
//! The real store on an in-memory database rather than a fake: billing one
//! call once when it is filed twice is a conditional write, and a mock of it
//! would only ever agree with itself.

use std::sync::Arc;

use alloy::primitives::B256;
use veil402_daemon::{
    adapters::SqliteStore,
    core::agent::{AgentError, AgentService},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const NOW: u64 = 1_900_000_000;

async fn ledger() -> Result<AgentService, Box<dyn std::error::Error>> {
    Ok(AgentService::new(Arc::new(
        SqliteStore::open("sqlite::memory:").await?,
    )))
}

/// Changes the last hex digit, which is the smallest a key can be wrong by.
fn tampered(key: &str) -> String {
    let (head, last) = key.split_at(key.len() - 1);
    format!("{head}{}", if last == "0" { "1" } else { "0" })
}

#[tokio::test]
async fn a_key_opens_only_the_agent_it_was_issued_for() -> Fallible {
    let ledger = ledger().await?;
    let issued = ledger.issue("aider", None, NOW).await?;
    let other = ledger.issue("cline", None, NOW).await?;

    assert_eq!(ledger.authenticate(&issued.key).await?.label, "aider");

    // The id is public and only the lookup; the secret is the whole proof.
    let borrowed = issued.key.replace(&issued.agent.id, &other.agent.id);
    assert!(matches!(
        ledger.authenticate(&borrowed).await,
        Err(AgentError::Unknown)
    ));

    for wrong in [
        tampered(&issued.key),
        "sk-proj-ZGVmaW5pdGVseU5vdE91cnM".to_owned(),
        String::new(),
    ] {
        assert!(
            matches!(ledger.authenticate(&wrong).await, Err(AgentError::Unknown)),
            "{wrong} opened something"
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_revoked_key_stops_opening_without_losing_what_it_spent() -> Fallible {
    let ledger = ledger().await?;
    let issued = ledger.issue("aider", None, NOW).await?;
    let agent = ledger.authenticate(&issued.key).await?;
    ledger
        .charge(
            ledger.reserve(&agent, 100)?,
            B256::repeat_byte(1),
            90,
            NOW,
        )
        .await?;

    assert!(ledger.revoke(&agent.id).await?);
    assert!(!ledger.revoke(&agent.id).await?, "revoking twice is no news");
    assert!(matches!(
        ledger.authenticate(&issued.key).await,
        Err(AgentError::Unknown)
    ));

    let listed = ledger.listed().await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].spent, 90, "the books outlive the key");
    assert_eq!(listed[0].calls, 1);
    assert_eq!(listed[0].used, Some(NOW));
    assert!(listed[0].revoked);
    Ok(())
}

#[tokio::test]
async fn a_budget_bounds_what_is_in_flight_as_well_as_what_is_spent() -> Fallible {
    let ledger = ledger().await?;
    let issued = ledger.issue("aider", Some(1_000), NOW).await?;
    let agent = ledger.authenticate(&issued.key).await?;

    let first = ledger.reserve(&agent, 600)?;
    // Nothing has been billed yet, and two of these would still overrun it.
    assert!(matches!(
        ledger.reserve(&agent, 600),
        Err(AgentError::Exhausted { .. })
    ));

    // A hold that is dropped is a hold given back.
    drop(first);
    let second = ledger.reserve(&agent, 600)?;
    ledger
        .charge(second, B256::repeat_byte(2), 250, NOW)
        .await?;

    let agent = ledger.authenticate(&issued.key).await?;
    assert_eq!(agent.spent, 250, "the bill, not the estimate");
    assert_eq!(agent.calls, 1);
    assert!(matches!(
        ledger.reserve(&agent, 800),
        Err(AgentError::Exhausted { budget: 1_000, committed: 250 })
    ));
    assert!(ledger.reserve(&agent, 700).is_ok());

    let open = ledger
        .authenticate(&ledger.issue("uncapped", None, NOW).await?.key)
        .await?;
    assert!(
        ledger.reserve(&open, u64::MAX).is_ok(),
        "no budget is no ceiling here"
    );
    Ok(())
}

/// A drain that outlived its reader, a retry, a restart: the enclave's digest
/// names the call rather than the attempt, so the second filing is the same
/// call arriving twice.
#[tokio::test]
async fn one_call_filed_twice_is_billed_once() -> Fallible {
    let ledger = ledger().await?;
    let issued = ledger.issue("aider", Some(1_000), NOW).await?;
    let agent = ledger.authenticate(&issued.key).await?;
    let attestation = B256::repeat_byte(7);

    ledger
        .charge(ledger.reserve(&agent, 100)?, attestation, 80, NOW)
        .await?;
    ledger
        .charge(ledger.reserve(&agent, 100)?, attestation, 80, NOW + 5)
        .await?;

    let agent = ledger.authenticate(&issued.key).await?;
    assert_eq!(agent.spent, 80);
    assert_eq!(agent.calls, 1);
    assert_eq!(agent.used, Some(NOW), "the second filing changed nothing");
    Ok(())
}
