use std::{convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::{StreamExt, future::ready, stream};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use serde::Deserialize;
use veil402_sdk::{
    Field,
    session::Sealed,
    wire::{
        AcceptBody, AgreeBody, CallBody, Offer, RefusedBody, SessionBody, SettlementBody, encode,
    },
};

use super::primitives::{Response, malformed};
use crate::core::{Billed, MeterError, MeteringService, Opened, Serving};

/// Carries the key every frame opens under. Sent once, before any of them.
const HEAD: &str = "head";

/// Carries what the call cost. The frames are sealed; this is not, because
/// this side is the one that has to bill it.
const BILL: &str = "bill";

/// How far ahead of a reader the relay may run. Bounded, so a slow client
/// still slows the relay down rather than filling memory with an answer
/// nobody is reading.
const FRAMES: usize = 32;

/// Says the stream ended on purpose. Not load-bearing — the last sealed
/// frame says so in a way the host cannot forge — but a reader without the
/// key still needs to know.
const DONE: &str = "[DONE]";

/// Names the call a retry repeats, so one served twice is billed once.
const IDEMPOTENCY: &str = "idempotency-key";

pub fn router(service: Arc<MeteringService>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { axum::http::StatusCode::OK }))
        .route("/v1/catalogue", get(catalogue))
        .route("/v1/sessions/agree", post(agree))
        .route("/v1/sessions/accept", post(accept))
        .route("/v1/sessions/{nullifier}/chat", post(chat))
        .route("/v1/sessions/{nullifier}/settle", post(settle))
        .with_state(service)
}

async fn catalogue(State(service): State<Arc<MeteringService>>) -> Response<Vec<Offer>> {
    service.offers().await.map_or_else(Into::into, Response::ok)
}

async fn agree(
    State(service): State<Arc<MeteringService>>,
    Json(body): Json<SessionBody>,
) -> Response<AgreeBody> {
    let Ok(session) = body.parse() else {
        return malformed("nullifier");
    };
    service.agree(&session).await.map_or_else(Into::into, Response::ok)
}

async fn accept(
    State(service): State<Arc<MeteringService>>,
    Json(body): Json<AcceptBody>,
) -> Response<()> {
    let (Ok(session), Ok(sealed)) = (body.session.parse(), body.sealed()) else {
        return malformed("delegation");
    };
    service
        .accept(&sealed, session)
        .await
        .map_or_else(Into::into, Response::ok)
}

async fn chat(
    State(service): State<Arc<MeteringService>>,
    Path(nullifier): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CallBody>,
) -> axum::response::Response {
    let Ok(nullifier) = veil402_sdk::wire::field(&nullifier) else {
        return malformed::<()>("nullifier").into_response();
    };
    let Ok(sealed) = body.sealed() else {
        return malformed::<()>("request").into_response();
    };
    let attempt = attempt(&headers);
    // Resolved before anything is streamed: once the status has gone there
    // is no way to send another, and a refusal is the daemon's cue to open a
    // different envelope.
    match service.meter(nullifier, &sealed, &attempt).await {
        Ok(Serving::Streaming(opened)) => {
            relayed(service, nullifier, attempt, sealed, opened).into_response()
        }
        Ok(Serving::Owing(challenge)) => Response::owing(challenge).into_response(),
        Err(MeterError::Upstream {
            status,
            retry_after,
            head,
            sealed,
        }) => Response::carrying(
            RefusedBody {
                retry_after,
                head: encode(&head),
                sealed: encode(&sealed),
            },
            axum::http::StatusCode::from_u16(status)
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY),
        )
        .into_response(),
        Err(error) => Response::<()>::from(error).into_response(),
    }
}

/// Relays frames this side cannot read, then says what they cost.
fn relayed(
    service: Arc<MeteringService>,
    nullifier: Field,
    attempt: String,
    asked: Sealed,
    opened: Opened,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let head = stream::once(ready(Ok(Event::default().event(HEAD).data(encode(&opened.head)))));

    // Driven rather than polled. A relay that only moves while someone is
    // reading it stops billing the moment a client gives up — and the
    // provider has already been paid for every token it produced. So the
    // call is drained to its end either way, and the reader gets whatever
    // of it is still wanted.
    let (sender, receiver) = mpsc::channel(FRAMES);
    tokio::spawn(async move {
        let mut frames = opened.frames;
        let mut listening = true;
        while let Some(billed) = frames.next().await {
            let event = match billed {
                Ok(Billed::Frame(bytes)) => Event::default().data(encode(&bytes)),
                Ok(Billed::Bill(metering)) => {
                    match service.billed(nullifier, &attempt, &asked, &metering).await {
                        Err(error) => broke(&error),
                        Ok(()) => Event::default()
                            .event(BILL)
                            .json_data(metering.metered)
                            .unwrap_or_else(|_| broke(&"the bill does not encode")),
                    }
                }
                Err(error) => broke(&error),
            };
            if listening && sender.send(Ok(event)).await.is_err() {
                listening = false;
            }
        }
        let _ = sender
            .send(Ok(Event::default().data(DONE)))
            .await;
    });

    Sse::new(head.chain(ReceiverStream::new(receiver)))
        // Both private models think before they speak, and a client with
        // nothing arriving eventually stops waiting.
        .keep_alive(KeepAlive::default())
}

/// A stream that ended badly says why. Correctness does not rest on it — the
/// buyer's last frame is sealed, so a stream cut short cannot pass as whole —
/// but a silent disconnection is nothing to debug with.
fn broke(error: &impl ToString) -> Event {
    let said = error.to_string();
    tracing::warn!(error = %said, "a stream ended early");
    Event::default().event("error").data(said)
}

#[derive(Debug, Default, Deserialize)]
struct Settling {
    /// Settle for a unit, to free a note nobody owes anything on.
    #[serde(default)]
    dust: bool,
}

async fn settle(
    State(service): State<Arc<MeteringService>>,
    Path(nullifier): Path<String>,
    Query(settling): Query<Settling>,
) -> Response<SettlementBody> {
    let Ok(nullifier) = veil402_sdk::wire::field(&nullifier) else {
        return malformed("nullifier");
    };
    service
        .settle(nullifier, settling.dust)
        .await
        .map_or_else(Into::into, Response::ok)
}

/// A caller that names no attempt gets no replay protection, which is its
/// choice to make: the path is the same either way.
fn attempt(headers: &HeaderMap) -> String {
    headers
        .get(IDEMPOTENCY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}
