//! What a relayed answer looks like on the wire.
//!
//! The daemon reads exactly this, so the shape is a contract rather than an
//! implementation detail: a key, then frames this side cannot open, then what
//! they cost.

mod support;

use std::sync::Arc;

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;
use veil402_gateway::{
    adapters::http::router,
    app::{Config, assemble},
    core::MeteringService,
};
use veil402_sdk::wire::{CallBody, Offer, encode};

use support::{DEADLINE, ROOT, asking, backing, delegate, pool};

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn gateway(
    frames: usize,
    breaks: bool,
) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    let state = tempfile::tempdir()?;
    Ok(assemble(
        &Config {
            pool: pool()?,
            root: ROOT,
            worker: "/nonexistent/worker".into(),
            artifacts: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../prover/veil402-gnark/artifacts/transaction-v4"),
            offers: vec![Offer {
                model: "stub".to_owned(),
                seller: alloy::primitives::Address::repeat_byte(0x5E),
                rate: 1,
            }],
            state: state.keep().join("sessions.redb"),
            bind: "127.0.0.1:0".parse()?,
            rpc: "http://127.0.0.1:0".to_owned(),
            upstream: "http://127.0.0.1:0".to_owned(),
            upstream_key: "sk-test".to_owned(),
            trust: "private".to_owned(),
            markup: 1,
            ceiling: 1_024,
        },
        backing(DEADLINE)?,
        Arc::new(support::Upstream { frames, breaks }),
    )?)
}

/// Posts one call and returns the status and whatever came back.
async fn chat(
    service: Arc<MeteringService>,
    nullifier: veil402_sdk::Field,
    sealed: &veil402_sdk::session::Sealed,
    attempt: &str,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let response = router(service)
        .oneshot(
            Request::post(format!("/v1/sessions/{}/chat", encode(&nullifier.to_bytes())))
                .header("content-type", "application/json")
                .header("idempotency-key", attempt)
                .body(Body::from(serde_json::to_vec(&CallBody::new(sealed))?))?,
        )
        .await?;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// The shape the daemon parses, and what asking twice gets it.
#[tokio::test]
async fn a_relayed_stream_is_billed_once_and_its_key_cannot_be_spent_again() -> Fallible {
    let service = gateway(3, false)?;
    let delegated = delegate(&service).await?;
    let asked = asking(&delegated.sealing, 40)?;

    let (status, stream) = chat(service.clone(), delegated.nullifier, &asked, "first").await?;
    assert_eq!(status, 200);

    let events: Vec<&str> = stream.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(events.first(), Some(&"event: head"), "the key did not come first");
    assert_eq!(
        events.iter().filter(|line| **line == "event: bill").count(),
        1,
        "a call was billed other than once"
    );
    assert_eq!(events.last(), Some(&"data: [DONE]"));
    assert!(
        events.iter().filter(|line| line.starts_with("data: ") && **line != "data: [DONE]").count()
            >= 4,
        "the three frames and their end did not all arrive"
    );
    assert!(
        !stream.contains("\"cost\":0"),
        "the bill said the call was free"
    );

    // Asking again buys the same work a second time, so it does not happen.
    // The idempotency draft would have the stored response replayed here;
    // this one was sealed to a key the buyer mints per attempt, so there is
    // nothing a retry could open. 409 says the key is spent, not busy.
    let (status, _) = chat(service.clone(), delegated.nullifier, &asked, "first").await?;
    assert_eq!(status, 409, "a call already served was served again");

    // One key, a different request — the case the draft is unambiguous
    // about, and a caller's bug rather than a race.
    let (status, _) = chat(
        service,
        delegated.nullifier,
        &asking(&delegated.sealing, 90)?,
        "first",
    )
    .await?;
    assert_eq!(status, 422, "a key was reused for another request");
    Ok(())
}

/// A stream that fails part-way says so rather than going quiet.
///
/// Correctness does not rest on it — the buyer's last frame is sealed as the
/// last, so a stream cut short can never pass as a whole one — but a silent
/// disconnection leaves whoever is debugging it nothing at all.
#[tokio::test]
async fn a_stream_that_breaks_says_why() -> Fallible {
    let service = gateway(2, true)?;
    let delegated = delegate(&service).await?;

    let (status, stream) = chat(
        service,
        delegated.nullifier,
        &asking(&delegated.sealing, 40)?,
        "broken",
    )
    .await?;

    // The status went out with the first byte, long before the break.
    assert_eq!(status, 200);
    assert!(stream.contains("event: error"), "a stream died without a word");
    assert!(
        !stream.contains("event: bill"),
        "a call that never finished was billed"
    );
    Ok(())
}
