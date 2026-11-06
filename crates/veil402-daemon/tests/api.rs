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
use common::{COST, Fallible, MODEL, serving};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use veil402_daemon::adapters::router;

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

async fn text(body: Body) -> Result<String, Box<dyn std::error::Error>> {
    Ok(String::from_utf8(body.collect().await?.to_bytes().to_vec())?)
}

#[tokio::test]
async fn an_agent_streams_in_openai_s_own_wire_shape_and_is_billed() -> Fallible {
    let fixture = serving(3).await?;
    let issued = fixture.agents.issue("aider", None, NOW).await?;

    let response = router(fixture.api())
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
    let issued = fixture.agents.issue("aider", None, NOW).await?;

    let response = router(fixture.api())
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
    let issued = fixture.agents.issue("aider", None, NOW).await?;
    fixture.agents.revoke(&issued.agent.id).await?;

    for key in ["", "sk-proj-somebody-elses", &issued.key] {
        let response = router(fixture.api()).oneshot(asking(key, true)).await?;
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
    let issued = fixture.agents.issue("aider", Some(10), NOW).await?;

    let response = router(fixture.api())
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
    let issued = fixture.agents.issue("cline", None, NOW).await?;

    let listed: Value = serde_json::from_str(
        &text(
            router(fixture.api())
                .oneshot(listing(&issued.key, "/v1/models"))
                .await?
                .into_body(),
        )
        .await?,
    )?;
    assert_eq!(listed["object"], "list");
    assert_eq!(listed["data"][0]["id"], MODEL);
    assert_eq!(listed["data"][0]["object"], "model");
    assert!(listed["data"][1].is_null(), "more models than are sold");

    let response = router(fixture.api())
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
        router(fixture.api()).oneshot(request).await?.status(),
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
    let issued = fixture.agents.issue("script", None, NOW).await?;

    let response = router(fixture.api())
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
