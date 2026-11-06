//! The agent API, over the daemon it actually drives.
//!
//! The router is exercised through `tower`, so what a test reads is what an
//! agent would read: the wire bytes, the status, and the headers an `OpenAI`
//! client makes decisions on.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{COST, Fallible, MODEL, OTHER, serving};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use veil402_daemon::{core::agent::Charge, core::envelope::EnclaveError};

const NOW: u64 = 1_900_000_000;

/// Enough to overrun the relay's buffer, so a reader that leaves must block
/// the drain — which is the only way a detached one can be told from one the
/// reader drives.
const MANY: usize = 128;

fn asking(key: &str, stream: bool) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            r#"{{"model":"{MODEL}","messages":[{{"role":"user","content":"hello"}}],"stream":{stream}}}"#
        )))
        .expect("request")
}

fn listing(key: &str, path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::empty())
        .expect("request")
}

/// The console surface takes no credential.
fn reading(path: &str) -> Request<Body> {
    Request::builder().uri(path).body(Body::empty()).expect("request")
}

async fn text(body: Body) -> Result<String, Box<dyn std::error::Error>> {
    Ok(String::from_utf8(body.collect().await?.to_bytes().to_vec())?)
}

#[tokio::test]
async fn an_agent_streams_in_openai_s_own_wire_shape_and_is_billed() -> Fallible {
    let fixture = serving(3).await?;
    let issued = fixture.agents.issue("aider", None, None, NOW).await?;

    let response = fixture.served()
        .oneshot(asking(&issued.key, true))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).expect("type"),
        "text/event-stream"
    );

    let body = text(response.into_body()).await?;
    let chunks: Vec<&str> = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .collect();
    assert_eq!(chunks.len(), 4, "three chunks and an end: {body}");
    assert_eq!(*chunks.last().expect("end"), "[DONE]");

    // Verbatim: what the upstream produced is what the agent reads, so a
    // field we have never heard of travels through untouched.
    let first: Value = serde_json::from_str(chunks[0])?;
    assert_eq!(first["choices"][0]["delta"]["content"], "0");
    assert_eq!(first["object"], "chat.completion.chunk");

    let agent = fixture.agents.authenticate(&issued.key).await?;
    assert_eq!(agent.spent, COST, "the call was not filed");
    assert_eq!(agent.calls, 1);
    Ok(())
}

/// The upstream has already been paid for every token it produced, so a
/// client that walks away stops the relay and not the bill.
#[tokio::test]
async fn a_disconnected_agent_is_billed_anyway() -> Fallible {
    let fixture = serving(MANY).await?;
    let issued = fixture.agents.issue("aider", None, None, NOW).await?;

    let response = fixture.served()
        .oneshot(asking(&issued.key, true))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    drop(response.into_body());

    // The drain is its own task now, so it finishes on its own schedule.
    for _ in 0..64 {
        if fixture.agents.authenticate(&issued.key).await?.spent == COST {
            return Ok(());
        }
        tokio::task::yield_now().await;
    }
    panic!("the answer was never billed");
}

#[tokio::test]
async fn a_key_that_was_never_issued_is_refused_in_openai_s_own_shape() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("aider", None, None, NOW).await?;
    fixture.agents.revoke(&issued.agent.id).await?;

    for key in ["", "sk-proj-somebody-elses", &issued.key] {
        let response = fixture.served().oneshot(asking(key, true)).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{key} was let in");
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).expect("challenge"),
            "Bearer"
        );
        let refusal: Value = serde_json::from_str(&text(response.into_body()).await?)?;
        assert_eq!(refusal["error"]["type"], "invalid_request_error");
        assert_eq!(refusal["error"]["code"], "invalid_api_key");
    }
    assert!(
        fixture.chain.sent.lock().expect("sent").is_empty(),
        "an unauthenticated call reached the chain"
    );
    Ok(())
}

/// A spent budget is a 429 because that is where `OpenAI` puts one, and a
/// 429 is retried unless the answer says otherwise.
#[tokio::test]
async fn an_agent_at_its_budget_is_told_not_to_retry() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("aider", Some(10), None, NOW).await?;

    let response = fixture.served()
        .oneshot(asking(&issued.key, true))
        .await?;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.headers().get("x-should-retry").expect("verdict"),
        "false"
    );
    let refusal: Value = serde_json::from_str(&text(response.into_body()).await?)?;
    assert_eq!(refusal["error"]["type"], "insufficient_quota");
    assert_eq!(refusal["error"]["code"], "budget_exhausted");
    assert!(
        fixture.chain.sent.lock().expect("sent").is_empty(),
        "a refused call still opened an envelope"
    );
    Ok(())
}

/// The catalogue is only what can be bought privately, so a model missing
/// from it is a model this daemon will not quietly buy in the clear.
#[tokio::test]
async fn a_model_the_daemon_cannot_buy_privately_is_not_offered() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("cline", None, None, NOW).await?;

    let listed: Value = serde_json::from_str(
        &text(
            fixture.served()
                .oneshot(listing(&issued.key, "/v1/models"))
                .await?
                .into_body(),
        )
        .await?,
    )?;
    assert_eq!(listed["object"], "list");
    assert_eq!(listed["data"][0]["object"], "model");
    let sold: Vec<&str> = listed["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|model| model["id"].as_str().expect("id"))
        .collect();
    assert_eq!(sold, [MODEL, OTHER], "the listing is not what is sold");

    let response = fixture.served()
        .oneshot(listing(&issued.key, "/v1/models/gpt-4o"))
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let refusal: Value = serde_json::from_str(&text(response.into_body()).await?)?;
    assert_eq!(refusal["error"]["code"], "model_not_found");

    let body = Body::from(r#"{"model":"gpt-4o","messages":[],"stream":true}"#);
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header(header::AUTHORIZATION, format!("Bearer {}", issued.key))
        .body(body)?;
    assert_eq!(
        fixture.served().oneshot(request).await?.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        fixture.chain.sent.lock().expect("sent").is_empty(),
        "a model we do not sell still opened an envelope"
    );
    Ok(())
}

#[tokio::test]
async fn an_answer_that_was_not_streamed_comes_back_whole() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("script", None, None, NOW).await?;

    let response = fixture.served()
        .oneshot(asking(&issued.key, false))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).expect("type"),
        "application/json"
    );

    let answer: Value = serde_json::from_str(&text(response.into_body()).await?)?;
    assert_eq!(answer["choices"][0]["delta"]["content"], "0");

    let agent = fixture.agents.authenticate(&issued.key).await?;
    assert_eq!(agent.spent, COST);
    assert_eq!(agent.calls, 1);
    Ok(())
}

/// An upstream's own error envelope. It already speaks the shape a client
/// parses, and `code` is what the clients agents are built on turn into
/// shrinking the context rather than retrying for ever.
const THEIRS: &str = r#"{"error":{"message":"your messages exceed the context window","type":"invalid_request_error","code":"context_length_exceeded"}}"#;

fn refused(status: u16, retry_after: Option<u64>) -> EnclaveError {
    EnclaveError::Upstream {
        status,
        retry_after,
        body: THEIRS.as_bytes().to_vec(),
    }
}

#[tokio::test]
async fn a_refusal_about_the_request_reaches_the_agent_whole() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("aider", None, None, NOW).await?;

    fixture.enclave.refusing(refused(400, None));
    let response = fixture.served()
        .oneshot(asking(&issued.key, true))
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        text(response.into_body()).await?,
        THEIRS,
        "the upstream's words were rewritten"
    );

    let agent = fixture.agents.authenticate(&issued.key).await?;
    assert_eq!(agent.spent, 0, "a refusal cost the agent something");
    assert_eq!(agent.calls, 0);
    Ok(())
}

/// Its own number rather than ours: a rate limit we guess at is one we hit
/// again.
#[tokio::test]
async fn a_rate_limit_carries_the_wait_the_upstream_asked_for() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("aider", None, None, NOW).await?;
    fixture.enclave.refusing(refused(429, Some(90)));

    let response = fixture.served()
        .oneshot(asking(&issued.key, true))
        .await?;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get(header::RETRY_AFTER).expect("wait"), "90");
    assert_eq!(text(response.into_body()).await?, THEIRS);
    Ok(())
}

/// A refusal about the seller's own account is not the agent's to act on:
/// told `insufficient_balance`, it would send its user to top up the wrong
/// one.
#[tokio::test]
async fn a_refusal_about_the_seller_is_not_told_in_the_seller_s_voice() -> Fallible {
    let fixture = serving(1).await?;
    let issued = fixture.agents.issue("aider", None, None, NOW).await?;
    fixture.enclave.refusing(EnclaveError::Upstream {
        status: 402,
        retry_after: None,
        body: br#"{"error":{"message":"the AI provider balance is empty","type":"insufficient_quota","code":"insufficient_balance"}}"#.to_vec(),
    });

    let response = fixture.served()
        .oneshot(asking(&issued.key, true))
        .await?;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = text(response.into_body()).await?;
    assert!(
        !body.contains("insufficient_balance"),
        "the seller's account reached the agent: {body}"
    );
    let refusal: Value = serde_json::from_str(&body)?;
    assert_eq!(refusal["error"]["code"], "upstream_unavailable");
    Ok(())
}

/// An audit shows what it was asked for and nothing beside it. Narrowing is
/// the whole claim — an auditor handed one agent's history must not learn
/// that another agent exists, let alone what it spent.
#[tokio::test]
async fn an_audit_narrows_to_one_agent_and_leaves_the_rest_out() -> Fallible {
    let fixture = serving(2).await?;
    let mine = fixture.agents.issue("mine", None, None, NOW).await?;
    let theirs = fixture.agents.issue("theirs", None, None, NOW).await?;

    // One real call, so a row carries what a real one would, and one filed
    // directly: the fixture holds a single note, and what is under test is
    // the narrowing rather than the envelope.
    let response = fixture.served().oneshot(asking(&mine.key, false)).await?;
    assert_eq!(response.status(), StatusCode::OK);

    let other = fixture.agents.authenticate(&theirs.key).await?;
    let hold = fixture.agents.reserve(&other, MODEL, COST)?;
    fixture
        .agents
        .charge(
            hold,
            &Charge {
                attestation: alloy::primitives::B256::repeat_byte(0xBB),
                cost: COST,
                at: NOW,
                nullifier: Some(veil402_sdk::Field::from(5_u64)),
                delivered: None,
                witness: None,
                sequence: 0,
                model: MODEL.to_owned(),
            },
        )
        .await?;

    let everything: Value = serde_json::from_str(
        &text(
            fixture
                .served()
                .oneshot(reading("/api/v1/audit"))
                .await?
                .into_body(),
        )
        .await?,
    )?;
    assert_eq!(
        everything["result"]["items"].as_array().expect("items").len(),
        2,
        "both calls were filed"
    );

    let narrowed: Value = serde_json::from_str(
        &text(
            fixture
                .served()
                .oneshot(reading(&format!("/api/v1/audit?agent={}", theirs.agent.id)))
                .await?
                .into_body(),
        )
        .await?,
    )?;
    let rows = narrowed["result"]["items"].as_array().expect("items");
    assert_eq!(rows.len(), 1, "the filter did not narrow");
    assert_eq!(rows[0]["label"], "theirs");
    assert_eq!(rows[0]["model"], MODEL, "the row names what was bought");
    assert!(
        rows[0]["proof"]["nullifier"].is_string(),
        "a row without its envelope cannot be tied to a settlement"
    );
    Ok(())
}
