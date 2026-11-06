//! What a relayed answer looks like on the wire.
//!
//! The daemon reads exactly this, so the shape is a contract rather than an
//! implementation detail: a key, then frames this side cannot open, then what
//! they cost.

mod support;

use std::sync::Arc;

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use futures_util::StreamExt;
use tower::ServiceExt;
use veil402_gateway::{
    adapters::http::router,
    app::{Config, assemble},
    core::MeteringService,
};
use veil402_sdk::wire::{CallBody, Delivered, MeteredBody, bytes, encode};

use support::{Asked, DEADLINE, ROOT, asked, asking, backing, delegate, pool};

type Fallible = Result<(), Box<dyn std::error::Error>>;

async fn gateway(
    frames: usize,
    breaks: bool,
) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    refusing(frames, breaks, None).await
}

async fn refusing(
    frames: usize,
    breaks: bool,
    refuses: Option<u16>,
) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    let state = tempfile::tempdir()?;
    Ok(assemble(
        &Config {
            serving_rpc: String::new(),
            serving: alloy::primitives::Address::ZERO,
            fee_cap: 20_000_000_000_000,
            pool: pool()?,
            root: Some(ROOT),
            enclave: None,
            worker: "/nonexistent/worker".into(),
            artifacts: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../prover/veil402-gnark/artifacts/transaction-v4"),
            seller: alloy::primitives::Address::repeat_byte(0x5E),
            state: state.keep().join("sessions.redb"),
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
        backing(DEADLINE)?,
        Arc::new(support::Upstream {
            frames,
            breaks,
            refuses,
        }),
    )
    .await?)
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
    let service = gateway(3, false).await?;
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

/// The bill accounts for the answer. Every sealed frame but the terminator,
/// folded as both ends fold them, so a seller billing for an answer it never
/// sent — or a relay that dropped one on the way — does not match.
#[tokio::test]
async fn a_bill_binds_the_frames_the_buyer_received() -> Fallible {
    let service = gateway(3, false).await?;
    let delegated = delegate(&service).await?;
    let Asked { sealed, exchange } = asked(&delegated.sealing, 40)?;

    let (status, stream) = chat(service, delegated.nullifier, &sealed, "bound").await?;
    assert_eq!(status, 200);

    // Read it the way the daemon reads it.
    let mut inbound = None;
    let mut delivered = Delivered::new();
    let mut billed: Option<MeteredBody> = None;
    let mut named = "";
    let mut folded = 0;
    for line in stream.lines().filter(|line| !line.is_empty()) {
        if let Some(name) = line.strip_prefix("event: ") {
            named = name;
            continue;
        }
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let event = core::mem::take(&mut named);
        match event {
            "head" => {
                let head: [u8; 32] = bytes(data)?.try_into().map_err(|_| "a malformed key")?;
                inbound = Some(exchange.inbound(head)?);
            }
            "bill" => billed = Some(serde_json::from_str(data)?),
            _ if data.trim() == "[DONE]" => {}
            _ => {
                let raw = bytes(data)?;
                let frame = inbound.as_mut().ok_or("a frame before its key")?.frame(&raw)?;
                if !frame.last {
                    delivered.frame(&raw);
                    folded += 1;
                }
            }
        }
    }

    assert_eq!(folded, 3, "the answer did not arrive whole");
    assert_eq!(
        billed.ok_or("a stream with no bill")?.delivered,
        Some(delivered.finish()),
        "the bill does not bind the answer that arrived"
    );
    Ok(())
}

/// A client that gives up half way has still been served, and the provider
/// has still been paid for every token it produced. The relay runs to its
/// end whether anyone is reading or not, because the bill is at the end.
#[tokio::test]
async fn a_client_that_leaves_still_pays_for_what_it_asked_for() -> Fallible {
    // More frames than the relay may run ahead of a reader, so the send
    // really does block on one that is no longer there. An answer small
    // enough to sit in the buffer would be delivered either way and prove
    // nothing.
    let service = gateway(64, false).await?;
    let delegated = delegate(&service).await?;
    let asked = asking(&delegated.sealing, 40)?;

    let response = router(service.clone())
        .oneshot(
            Request::post(format!(
                "/v1/sessions/{}/chat",
                encode(&delegated.nullifier.to_bytes())
            ))
            .header("content-type", "application/json")
            .header("idempotency-key", "abandoned")
            .body(Body::from(serde_json::to_vec(&CallBody::new(&asked))?))?,
        )
        .await?;

    // Read the key, then walk away mid-answer.
    let mut body = response.into_body().into_data_stream();
    body.next().await;
    drop(body);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // What the session owes is the only honest witness: a second call would
    // bill itself, so asking whether *it* was recorded says nothing. The
    // running total either carries the abandoned call or it does not.
    let (status, stream) = chat(service, delegated.nullifier, &asked, "stayed").await?;
    assert_eq!(status, 200);
    let bill = billed(&stream)?;

    assert_eq!(
        bill.total,
        bill.cost * 2,
        "the call nobody stayed for was never billed"
    );
    Ok(())
}

/// The bill a relayed stream ended on.
fn billed(stream: &str) -> Result<MeteredBody, Box<dyn std::error::Error>> {
    let mut lines = stream.lines();
    while let Some(line) = lines.next() {
        if line == "event: bill" {
            let data = lines.next().ok_or("a bill with no data")?;
            return Ok(serde_json::from_str(
                data.strip_prefix("data: ").ok_or("a bill that is not data")?,
            )?);
        }
    }
    Err("the stream carried no bill".into())
}

/// A stream that fails part-way says so rather than going quiet.
///
/// Correctness does not rest on it — the buyer's last frame is sealed as the
/// last, so a stream cut short can never pass as a whole one — but a silent
/// disconnection leaves whoever is debugging it nothing at all.
#[tokio::test]
async fn a_stream_that_breaks_says_why() -> Fallible {
    let service = gateway(2, true).await?;
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

/// A margin under a doubling has to be expressible, and admission has to use
/// the same arithmetic that bills: a call let through on one sum and charged
/// on another is a seller quietly paying the difference.
#[test]
fn what_a_call_is_admitted_for_is_what_it_is_billed() {
    use veil402_gateway::core::Pricing;

    let at_cost = Pricing::new(1, 0);
    assert_eq!(at_cost.charge(233), 233);
    assert_eq!(at_cost.affords(233), 233);

    // Ten per cent over, which no whole-number multiplier could say.
    let tenth = Pricing::new(1, 1_000);
    assert_eq!(tenth.charge(1_000), 1_100);
    assert_eq!(tenth.affords(1_100), 1_000);

    // An eighteen-decimal token, where the scale alone is a trillion.
    let wei = Pricing::new(1_000_000_000_000, 2_500);
    assert_eq!(wei.charge(1), 1_250_000_000_000);
    assert_eq!(wei.affords(1_250_000_000_000), 1);
}

/// A refusal is an answer of a kind, and this side is not entitled to read
/// it either. The status travels plainly, because it says nothing about the
/// prompt; the words travel sealed, because they could.
#[tokio::test]
async fn an_upstream_refusal_keeps_its_status_and_loses_none_of_its_words() -> Fallible {
    let service = refusing(0, false, Some(400)).await?;
    let delegated = delegate(&service).await?;
    let asked = support::asked(&delegated.sealing, 40)?;

    let (status, body) = chat(service, delegated.nullifier, &asked.sealed, "call").await?;

    assert_eq!(status, 400, "the upstream's own verdict");
    assert!(
        !body.contains("context_length_exceeded"),
        "the host read the refusal: {body}"
    );
    let reply: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(reply["action"], "fail", "nothing about retrying helps");
    let carried = &reply["result"];
    assert_eq!(carried["retry_after"], 42, "the upstream's own number");
    assert_eq!(
        carried["head"].as_str().map(str::len),
        Some(64),
        "no key to open the words with"
    );

    // And the buyer, who holds the other half, reads them exactly.
    let head: [u8; 32] = veil402_sdk::wire::bytes(carried["head"].as_str().unwrap_or_default())?
        .try_into()
        .map_err(|_| "a malformed key")?;
    let opened = asked
        .exchange
        .inbound(head)?
        .frame(&veil402_sdk::wire::bytes(
            carried["sealed"].as_str().unwrap_or_default(),
        )?)?;
    assert_eq!(opened.payload, support::REFUSAL.as_bytes());
    assert!(opened.last, "a refusal is the whole of what there is to say");
    Ok(())
}
