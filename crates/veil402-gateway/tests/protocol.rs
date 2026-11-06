//! The contract the daemon reads: what to pay, and what to do when refused.

mod support;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use veil402_gateway::{
    adapters::http::router,
    app::{Config, assemble},
    core::MeteringService,
};
use veil402_sdk::{
    Field,
    evm::PoolClient,
    wire::{Action, Challenge, Reply, encode},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn seller() -> alloy::primitives::Address {
    alloy::primitives::Address::repeat_byte(0x5E)
}

fn artifacts() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../prover/veil402-gnark/artifacts/transaction-v4")
}

/// A gateway holding no sessions, which is every gateway at boot. The worker
/// is never started: nothing here proves anything.
fn gateway() -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    let state = tempfile::tempdir()?;
    Ok(assemble(
        &Config {
            state: state.keep().join("sessions.redb"),
            rpc: "http://127.0.0.1:0".to_owned(),
            pool: PoolClient::new(
                "0x42532488f967329e2B090dCf68396Ef0dd63de69".parse()?,
                "0x0000000000000000000000000000000000000abc".parse()?,
                "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC".parse()?,
                296,
            )?,
            root: [3; 32],
            worker: PathBuf::from("/nonexistent/worker"),
            artifacts: artifacts(),
            seller: seller(),
            bind: "127.0.0.1:0".parse()?,
            upstream: "http://127.0.0.1:0".to_owned(),
            upstream_key: "sk-test".to_owned(),
            trust: "private".to_owned(),
            markup: 1,
            ceiling: 1_024,
        },
        Arc::new(support::Pool::empty()),
        Arc::new(support::Upstream { frames: 2, breaks: false }),
    )?)
}

async fn call(gateway: Arc<MeteringService>, path: &str, body: &str) -> (StatusCode, Vec<u8>) {
    let response = router(gateway)
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec();
    (status, bytes)
}

/// A buyer with no envelope is told what it would take to get one, not that
/// something went wrong.
#[tokio::test]
async fn a_call_without_an_envelope_is_challenged_to_pay() -> Fallible {
    let nullifier = encode(&Field::from(7_u64).to_bytes());
    let (status, body) = call(
        gateway()?,
        &format!("/v1/sessions/{nullifier}/chat"),
        // Well formed but undecryptable, which is the point: no envelope
        // means nobody tries to open it, and the answer is the price.
        &format!(r#"{{"encapsulated":"{}","ciphertext":"00"}}"#, "ab".repeat(32)),
    )
    .await;

    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    let reply: Reply<Challenge> = serde_json::from_slice(&body)?;
    assert_eq!(reply.action, Some(Action::Pay), "the answer is to open an envelope");
    let challenge = reply.result.ok_or("no challenge")?;
    assert_eq!(challenge.scheme, "escrow");
    assert_eq!(challenge.seller, seller());
    assert_eq!(challenge.chain_id, 296);
    Ok(())
}

/// Every refusal carries what the daemon should do next. A status code says
/// what went wrong; only this says whether to retry, re-shop, or give up.
#[tokio::test]
async fn a_refusal_says_what_to_do_next() -> Fallible {
    let nullifier = encode(&Field::from(7_u64).to_bytes());

    // A settlement for a session this gateway does not hold: the enclave
    // restarted, the escrow still stands, so handing it over again repairs it.
    let (status, body) = call(
        gateway()?,
        &format!("/v1/sessions/{nullifier}/settle"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let reply: Reply<()> = serde_json::from_slice(&body)?;
    assert_eq!(reply.action, Some(Action::Retry));

    // A nullifier that is not a field element is the caller's bug.
    let (status, body) = call(gateway()?, "/v1/sessions/zzzz/settle", "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_slice::<Reply<()>>(&body)?.action,
        Some(Action::Fail)
    );
    Ok(())
}
