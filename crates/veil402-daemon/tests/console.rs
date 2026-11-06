//! The operator surface, over the daemon it actually drives.
//!
//! Only what can regress: the token that separates the two surfaces, the
//! mandate allowlist that gates a call before it costs anything, pausing,
//! and the arithmetic that splits one shared envelope between agents.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{COST, Fallible, MODEL, OTHER, serving};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use veil402_daemon::core::{
    agent::Charge,
    envelope::EnvelopeStore,
    index::{Batch, IndexStore, Locked},
};

const NOW: u64 = 1_900_000_000;

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("request")
}

fn post(path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("request")
}

fn asking(key: &str, model: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            r#"{{"model":"{model}","messages":[{{"role":"user","content":"hi"}}],"stream":false}}"#
        )))
        .expect("request")
}

async fn json(body: Body) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&body.collect().await?.to_bytes())?)
}

/// A mandate that names its models refuses everything else, and refuses it
/// before the call costs anything.
#[tokio::test]
async fn a_mandate_buys_only_the_models_it_names() -> Fallible {
    let fixture = serving(2).await?;
    let granted: Value = json(
        fixture
            .served()
            .oneshot(post("/api/v1/agents",
                &format!(r#"{{"label":"scoped","budget":100000,"models":["{MODEL}"]}}"#),
            ))
            .await?
            .into_body(),
    )
    .await?;
    let key = granted["result"]["key"].as_str().expect("key").to_owned();

    let refused = fixture.served().oneshot(asking(&key, OTHER)).await?;
    assert_eq!(
        refused.status(),
        StatusCode::FORBIDDEN,
        "a mandate bought a model it does not cover"
    );
    let said = json(refused.into_body()).await?;
    assert_eq!(said["error"]["code"], "model_not_permitted");

    let spent: Value = json(
        fixture
            .served()
            .oneshot(get("/api/v1/agents"))
            .await?
            .into_body(),
    )
    .await?;
    let mandate = &spent["result"]["items"][0];
    assert_eq!(mandate["spent"], 0, "a refused call was billed");

    assert_eq!(
        fixture.served().oneshot(asking(&key, MODEL)).await?.status(),
        StatusCode::OK,
        "the model the mandate names was refused"
    );
    Ok(())
}

/// Pausing stops a key without closing the one-way door; resuming lets it
/// run again. Revoking is the door, and a revoked key cannot be resumed.
#[tokio::test]
async fn a_paused_mandate_stops_and_starts_again_but_a_revoked_one_does_not() -> Fallible {
    let fixture = serving(3).await?;
    let agent = fixture.agents.issue("aider", None, None, NOW).await?;
    let id = agent.agent.id.clone();

    let paused = fixture
        .served()
        .oneshot(post(&format!("/api/v1/agents/{id}/pause"), ""))
        .await?;
    assert_eq!(paused.status(), StatusCode::OK);
    assert_eq!(
        fixture.served().oneshot(asking(&agent.key, MODEL)).await?.status(),
        StatusCode::UNAUTHORIZED,
        "a paused key still bought a call"
    );

    fixture
        .served()
        .oneshot(post(&format!("/api/v1/agents/{id}/resume"), ""))
        .await?;
    assert_eq!(
        fixture.served().oneshot(asking(&agent.key, MODEL)).await?.status(),
        StatusCode::OK,
        "a resumed key stayed stopped"
    );

    fixture.agents.revoke(&id).await?;
    let resumed: Value = json(
        fixture
            .served()
            .oneshot(post(&format!("/api/v1/agents/{id}/resume"), ""))
            .await?
            .into_body(),
    )
    .await?;
    assert_eq!(
        resumed["result"]["changed"], false,
        "resuming walked back a revocation"
    );
    Ok(())
}

/// One envelope serves every agent, so the per-agent shares are a view of a
/// single ceiling. They must add up to it, and no agent's meter may read
/// past its own share.
#[tokio::test]
async fn the_shared_envelope_splits_into_shares_that_add_up_to_its_ceiling() -> Fallible {
    let fixture = serving(2).await?;
    let first = fixture.agents.issue("first", None, None, NOW).await?;
    let second = fixture.agents.issue("second", None, None, NOW).await?;

    // One real call opens the envelope; the second agent's is filed against
    // the same nullifier, which is what sharing one ceiling looks like.
    fixture.served().oneshot(asking(&first.key, MODEL)).await?;
    // Billing is its own task, so the row lands on its own schedule.
    for _ in 0..64 {
        if fixture.agents.authenticate(&first.key).await?.spent > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    // The lock has landed; the index seeing it is what dates the envelope,
    // and the fake chain emits no events of its own.
    let nullifier = fixture.store.newest().await?.expect("an envelope was opened");
    fixture
        .store
        .append(
            &Batch {
                locks: vec![Locked {
                    nullifier,
                    seller: common::seller(),
                    enclave: common::seller(),
                    deadline: NOW + 600,
                    block: 1,
                }],
                ..Batch::default()
            },
            &[],
        )
        .await?;
    let open = fixture.service.current().await?.expect("an open envelope");

    let other = fixture.agents.authenticate(&second.key).await?;
    let hold = fixture.agents.reserve(&other, MODEL, COST)?;
    fixture
        .agents
        .charge(
            hold,
            &Charge {
                attestation: alloy::primitives::B256::repeat_byte(0xCC),
                cost: COST,
                at: NOW,
                nullifier: Some(open.nullifier),
                delivered: None,
                witness: None,
                sequence: 1,
                model: MODEL.to_owned(),
            },
        )
        .await?;

    let envelope: Value = json(
        fixture
            .served()
            .oneshot(get("/api/v1/envelope"))
            .await?
            .into_body(),
    )
    .await?;
    let shown = &envelope["result"];
    let ceiling = shown["ceiling"].as_u64().expect("ceiling");
    let agents = shown["agents"].as_array().expect("agents");
    assert_eq!(agents.len(), 2, "both agents drew on the one envelope");

    let shares: u64 = agents
        .iter()
        .map(|row| row["escrow"].as_u64().expect("escrow"))
        .sum();
    assert_eq!(
        shares, ceiling,
        "the shares do not add up to the one real ceiling"
    );
    for row in agents {
        assert!(
            row["metered"].as_u64() <= row["escrow"].as_u64(),
            "an agent's meter read past its own share"
        );
    }
    assert_eq!(
        shown["spent"].as_u64().expect("spent"),
        agents
            .iter()
            .map(|row| row["metered"].as_u64().expect("metered"))
            .sum::<u64>(),
        "the envelope's total disagrees with the rows under it"
    );
    Ok(())
}
