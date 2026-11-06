//! The router adapter, against bytes a real router actually sent.
//!
//! The fixtures are captures, not inventions: a reasoning model's answer in
//! both shapes, and three catalogue entries that between them cover a model
//! whose weights run in an enclave, one that only proxies to a vendor, and
//! one that is not chat at all.

use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::Address,
    signers::{SignerSync, local::PrivateKeySigner},
};
use axum::{
    Json, Router as Web,
    extract::Path,
    http::{StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
    routing::{get, post},
};
use futures_util::StreamExt;
use veil402_enclave::{Ask, Attested, Attestor, Served, Upstream, adapters::Router};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const ANSWER: &str = include_str!("fixtures/chat-completion.json");
const STREAM: &str = include_str!("fixtures/chat-stream.sse");
const MODELS: &str = include_str!("fixtures/models.json");

const MODEL: &str = "0gm-1.0-35b-a3b";

/// Who the fixtures say served the call.
const PROVIDER: &str = "0x4870CbC4D07d6Ac2EE5aA865588e5985FE77a4E9";

/// What the router calls the answer, and the provider knows it by.
const SERVED: &str = "res-7f3a";

/// Standing in for the answer the provider signed over.
const SIGNED: &str = "a reasoning model's answer, verbatim";

/// What a request with no limit of its own is allowed to spend.
const CEILING: u32 = 1_024;

async fn completions(body: axum::body::Bytes) -> axum::response::Response {
    let asked: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let key = [("zg-res-key", SERVED)];
    if asked.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        ([(CONTENT_TYPE, "text/event-stream")], key, STREAM).into_response()
    } else {
        ([(CONTENT_TYPE, "application/json")], key, ANSWER).into_response()
    }
}

/// A router serving what the real one served, on a port nothing else holds.
async fn upstream(
    vouched: HashMap<Address, Attestor>,
) -> Result<Router, Box<dyn std::error::Error>> {
    let web = Web::new()
        .route("/models", get(|| async { ([(CONTENT_TYPE, "application/json")], MODELS) }))
        .route("/chat/completions", post(completions));
    Ok(Router::new(&serving(web).await?, "sk-test", "private", CEILING, Arc::new(vouched))?)
}

/// A provider that will say what it signed, for the one call it served.
async fn provider(key: PrivateKeySigner) -> Result<String, Box<dyn std::error::Error>> {
    let signature = key.sign_message_sync(SIGNED.as_bytes())?;
    let signed = serde_json::json!({
        "text": SIGNED,
        "signature": format!("0x{}", alloy::hex::encode(signature.as_bytes())),
    });
    serving(Web::new().route(
        "/v1/proxy/signature/{served}",
        // Only for the key the router named: asking for anything else is
        // asking about a call this provider did not serve.
        get(move |Path(served): Path<String>| {
            let signed = signed.clone();
            async move {
                if served == SERVED {
                    Json(signed).into_response()
                } else {
                    StatusCode::NOT_FOUND.into_response()
                }
            }
        }),
    ))
    .await
}

async fn serving(web: Web) -> Result<String, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        let _ = axum::serve(listener, web).await;
    });
    Ok(base)
}

fn request(stream: bool) -> Vec<u8> {
    format!(
        r#"{{"model":"{MODEL}","messages":[{{"role":"user","content":"hi"}}],"max_tokens":200,"stream":{stream}}}"#
    )
    .into_bytes()
}

async fn served(upstream: &Router, stream: bool) -> Result<Vec<Served>, Box<dyn std::error::Error>> {
    let answer = upstream
        .ask(Ask {
            model: MODEL.to_owned(),
            body: request(stream),
            budget: 1_000_000,
        })
        .await?;
    Ok(answer.collect::<Vec<_>>().await.into_iter().collect::<Result<_, _>>()?)
}

/// Both shapes end the same way: the frames the buyer asked for, then what
/// the call cost — and the bill never travels on as a frame, because it
/// states our price rather than theirs.
#[tokio::test]
async fn an_answer_yields_the_bill_it_carries_and_relays_the_rest() -> Fallible {
    let upstream = upstream(HashMap::new()).await?;

    for (stream, cost, reference, completion) in [
        (false, 61, "d763051d-7fee-49d6-ae52-17fce8600f7a", 120),
        (true, 100, "0c1f943f-706a-4c3e-84c2-98d5595143c7", 200),
    ] {
        let served = served(&upstream, stream).await?;
        let (frames, done) = served.split_at(served.len() - 1);

        let Some(Served::Done(receipt)) = done.first() else {
            panic!("a call ended without a bill");
        };
        assert_eq!(receipt.cost, cost, "micro-USD, as the router billed it");
        assert_eq!(receipt.reference, reference);
        assert_eq!(receipt.provider, Some(PROVIDER.parse()?));
        assert_eq!(receipt.tokens.completion, completion);
        assert_eq!(
            receipt.attested,
            Attested::Absent,
            "a router cannot sign for the provider it relays to"
        );

        assert!(!frames.is_empty(), "nothing was relayed");
        for frame in frames {
            let Served::Frame(bytes) = frame else {
                panic!("a bill arrived before the end");
            };
            assert!(
                !String::from_utf8_lossy(bytes).contains("x_0g_trace"),
                "our cost was relayed to the buyer"
            );
        }
    }
    Ok(())
}

/// Attested is not one thing: a model whose weights run in an enclave can
/// carry the claim, and one that terminates TLS in an enclave and forwards
/// the prompt to a vendor cannot.
#[tokio::test]
async fn a_catalogue_separates_what_can_be_private_from_what_cannot() -> Fallible {
    let listed = upstream(HashMap::new()).await?.catalogue().await?;

    assert_eq!(
        listed.iter().map(|l| l.model.as_str()).collect::<Vec<_>>(),
        [MODEL, "claude-sonnet-5"],
        "an image model was listed as chat"
    );

    let confidential = &listed[0];
    assert!(confidential.confidential);
    assert_eq!(confidential.prompt, 80_000, "micro-USD per million tokens");
    assert_eq!(confidential.completion, 480_000);
    assert_eq!(confidential.max_output, 32_768);

    assert!(!listed[1].confidential, "a proxied vendor passed as private");
    assert_eq!(listed[1].completion, 10_000_000);
    Ok(())
}

/// The bill arrives after the work, so the only moment to refuse a call this
/// session cannot pay for is before it is sent.
#[tokio::test]
async fn a_call_that_could_overrun_never_starts() -> Fallible {
    let upstream = upstream(HashMap::new()).await?;
    let listed = upstream.catalogue().await?;
    let confidential = &listed[0];

    // 200 completion tokens at 480_000 per million is 96; the prompt, counted
    // high on purpose, adds 4.
    assert_eq!(confidential.rates().most(&request(false), CEILING), 100);
    // No limit of its own, so the configured one applies.
    assert_eq!(
        confidential.rates().most(br#"{"model":"x"}"#, CEILING),
        u64::from(CEILING) * 480_000 / 1_000_000
    );

    let refused = upstream
        .ask(Ask {
            model: MODEL.to_owned(),
            body: request(true),
            budget: 99,
        })
        .await;
    assert!(
        matches!(refused, Err(veil402_enclave::UpstreamError::Unaffordable)),
        "a call was sent that the session could not pay for"
    );
    Ok(())
}

/// A verdict is the provider's own signature, recovered against the key its
/// quote bound to hardware — so a provider signing with any other key is a
/// failure rather than a shrug, and the router relaying it cannot say
/// otherwise. Both shapes ask, and both ask only after the answer is out.
#[tokio::test]
async fn an_answer_is_attested_only_when_the_quoted_key_signed_it() -> Fallible {
    let key = PrivateKeySigner::random();
    let endpoint = provider(key.clone()).await?;

    for (signer, verdict) in [
        (key.address(), Attested::Verified),
        (PrivateKeySigner::random().address(), Attested::Failed),
    ] {
        let vouched = HashMap::from([(
            PROVIDER.parse()?,
            Attestor { endpoint: endpoint.clone(), signer },
        )]);
        let upstream = upstream(vouched).await?;

        for stream in [false, true] {
            let served = served(&upstream, stream).await?;
            let Some(Served::Done(receipt)) = served.last() else {
                panic!("a call ended without a bill");
            };
            assert_eq!(receipt.attested, verdict, "streaming: {stream}");
        }
    }
    Ok(())
}

/// The real thing, when a key is there to use. Costs a fraction of a cent,
/// so it is opt-in rather than part of a run.
#[tokio::test]
#[ignore = "spends real credit; needs VEIL_0G_KEY and VEIL_0G_BASE"]
async fn a_real_model_streams_and_reports_what_it_cost() -> Fallible {
    let upstream = Router::new(
        &std::env::var("VEIL_0G_BASE")?,
        &std::env::var("VEIL_0G_KEY")?,
        "private",
        CEILING,
        Arc::new(HashMap::new()),
    )?;
    let listed = upstream.catalogue().await?;
    let model = listed
        .iter()
        .find(|listing| listing.confidential)
        .ok_or("the catalogue offers nothing confidential")?;

    let body = format!(
        r#"{{"model":"{}","messages":[{{"role":"user","content":"Count to three."}}],"max_tokens":256,"stream":true,"stream_options":{{"include_usage":true}}}}"#,
        model.model
    );
    let served: Vec<Served> = upstream
        .ask(Ask {
            model: model.model.clone(),
            body: body.into_bytes(),
            budget: model.rates().most(&[], CEILING) * 2,
        })
        .await?
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_, _>>()?;

    let Some(Served::Done(receipt)) = served.last() else {
        panic!("the stream ended without a bill");
    };
    assert!(served.len() > 2, "an answer arrived in one piece");
    assert!(receipt.cost > 0, "a call that cost nothing");
    assert!(!receipt.reference.is_empty());
    assert!(receipt.provider.is_some());
    assert_eq!(receipt.attested, Attested::Absent);
    Ok(())
}
