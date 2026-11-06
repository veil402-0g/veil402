//! The agent API: `OpenAI`'s shape over a shielded note.

use std::{convert::Infallible, sync::Arc};

use axum::{
    Json, RequestPartsExt, Router,
    body::Bytes,
    extract::{FromRequestParts, Path, State},
    http::{HeaderMap, header::CONTENT_TYPE, request::Parts},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use futures_util::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use alloy::hex;

use super::primitives::Refusal;
use crate::{
    app::now,
    core::{
        agent::{Agent, AgentService, Charge, Hold},
        envelope::{EnclaveError, EnvelopeError, EnvelopeService, Metered, Piece, collected},
        index::IndexService,
    },
};

/// How far ahead of a reader the relay may run. Bounded, so a slow client
/// slows the relay rather than filling memory with an answer nobody reads.
const FRAMES: usize = 64;

/// Says the stream ended on purpose, by convention rather than framing.
const DONE: &str = "[DONE]";

/// Names the call a retry repeats, so one served twice is billed once.
const IDEMPOTENCY: &str = "idempotency-key";

/// Everything the agent API is served over.
#[derive(Clone)]
pub struct Api {
    pub agents: Arc<AgentService>,
    pub envelopes: Arc<EnvelopeService>,
    pub index: Arc<IndexService>,
    /// What a call may produce when the request names no limit of its own.
    /// Only the reservation rests on it; the bill is whatever arrives.
    pub cap: u32,
    /// Stamped on every model listed, which is where clients show an age.
    pub started: u64,
}

pub fn router(api: Api) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(completions))
        .route("/v1/models", get(models))
        .route("/v1/models/{model}", get(model))
        .route("/healthz", get(healthz))
        .with_state(api)
}

/// The agent a request authenticates as.
///
/// An extractor rather than a line in each handler: a route that forgets to
/// authenticate does not compile.
struct Caller(Agent);

impl FromRequestParts<Api> for Caller {
    type Rejection = Refusal;

    async fn from_request_parts(parts: &mut Parts, api: &Api) -> Result<Self, Refusal> {
        let bearer = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| Refusal::unauthenticated())?;
        Ok(Self(api.agents.authenticate(bearer.token()).await?))
    }
}

/// The two fields this side reads. Everything else goes upstream untouched,
/// and serde ignores what it does not know, so whatever `OpenAI` adds next
/// travels with it.
#[derive(Deserialize)]
struct Asked {
    model: String,
    #[serde(default)]
    stream: bool,
}

async fn completions(
    State(api): State<Api>,
    Caller(agent): Caller,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Refusal> {
    let asked: Asked = serde_json::from_slice(&body)
        .map_err(|error| Refusal::malformed(error.to_string()))?;
    let offer = api
        .envelopes
        .offer(&asked.model)
        .await?
        .ok_or_else(|| Refusal::unlisted(&asked.model))?;

    let hold = api
        .agents
        .reserve(&agent, &asked.model, offer.rates().most(&body, api.cap))?;
    let pieces = api.envelopes.dispatch(&body, &attempt(&headers)).await?;

    if asked.stream {
        Ok(streaming(api, agent.id, hold, pieces, asked.model))
    } else {
        whole(api, agent.id, hold, pieces, asked.model).await
    }
}

/// Relays an answer as the chunks it arrived in.
fn streaming(
    api: Api,
    agent: String,
    hold: Hold,
    pieces: BoxStream<'static, Result<Piece, EnclaveError>>,
    model: String,
) -> Response {
    let (sender, receiver) = mpsc::channel(FRAMES);
    tokio::spawn(relaying(api, agent, hold, pieces, sender, model));
    Sse::new(ReceiverStream::new(receiver))
        // A model that thinks before it speaks says nothing for a while, and
        // a client with nothing arriving eventually stops waiting.
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Driven rather than polled.
///
/// The upstream has already been paid for every token it produced, so a
/// client that hangs up stops the relay and not the bill.
async fn relaying(
    api: Api,
    agent: String,
    hold: Hold,
    mut pieces: BoxStream<'static, Result<Piece, EnclaveError>>,
    sender: mpsc::Sender<Result<Event, Infallible>>,
    model: String,
) {
    let mut hold = Some(hold);
    let mut listening = true;
    while let Some(piece) = pieces.next().await {
        let event = match piece {
            Ok(Piece::Body(bytes)) => Event::default().data(String::from_utf8_lossy(&bytes)),
            // Ours, not the agent's: it prices an envelope the agent never
            // sees. What the agent is told about tokens came from the
            // upstream and has already gone past in a chunk of its own.
            Ok(Piece::Billed(metered)) => {
                bill(&api, &agent, &mut hold, &metered, &model).await;
                continue;
            }
            // One frame carrying the failure, then the end: a stream that
            // stops without either leaves a client waiting on nothing.
            Err(error) => broke(&error),
        };
        if listening && sender.send(Ok(event)).await.is_err() {
            listening = false;
        }
    }
    let _ = sender.send(Ok(Event::default().data(DONE))).await;
}

/// Reads a whole answer before returning it, and bills it either way.
///
/// Spawned rather than awaited in place: a client that gives up drops the
/// handler, and the call still has to be paid for.
async fn whole(
    api: Api,
    agent: String,
    hold: Hold,
    pieces: BoxStream<'static, Result<Piece, EnclaveError>>,
    asked: String,
) -> Result<Response, Refusal> {
    let (tell, told) = oneshot::channel();
    tokio::spawn(async move {
        let answered = collected(pieces).await;
        if let Ok(answered) = &answered {
            bill(&api, &agent, &mut Some(hold), &answered.metered, &asked).await;
        }
        let _ = tell.send(answered);
    });
    let answered = told
        .await
        .map_err(|_| Refusal::internal("the answer was dropped"))?
        .map_err(EnvelopeError::from)?;
    Ok((
        [(CONTENT_TYPE, "application/json")],
        answered.completion,
    )
        .into_response())
}

async fn bill(api: &Api, agent: &str, hold: &mut Option<Hold>, metered: &Metered, model: &str) {
    let Some(hold) = hold.take() else { return };
    let charge = Charge {
        attestation: metered.attestation,
        cost: metered.cost,
        at: now(),
        nullifier: metered.nullifier,
        delivered: metered.delivered,
        witness: metered.witness.clone(),
        sequence: metered.sequence,
        model: model.to_owned(),
    };
    if let Err(error) = api.agents.charge(hold, &charge).await {
        // The envelope paid for this either way. What is lost is knowing
        // whose it was.
        tracing::error!(%error, agent, cost = metered.cost, "a served call was not filed");
    }
}

/// A stream that ended badly says so in the shape a client already reads.
fn broke(error: &impl ToString) -> Event {
    let said = error.to_string();
    tracing::warn!(error = %said, "a stream ended early");
    Event::default().data(
        serde_json::json!({ "error": { "message": said, "type": "api_error" } }).to_string(),
    )
}

/// A caller that names no attempt gets one minted, so an answer lost on the
/// way back is refused on the retry rather than bought a second time.
fn attempt(headers: &HeaderMap) -> String {
    if let Some(named) = headers.get(IDEMPOTENCY).and_then(|value| value.to_str().ok()) {
        return named.to_owned();
    }
    let mut minted = [0_u8; 16];
    // One key shared by every call would make the second look like a repeat,
    // so no key at all is the safer failure.
    getrandom::fill(&mut minted).map_or_else(|_| String::new(), |()| hex::encode(minted))
}

#[derive(Serialize)]
struct Listed {
    object: &'static str,
    data: Vec<Model>,
}

#[derive(Serialize)]
struct Model {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: &'static str,
}

impl Api {
    fn listing(&self, id: String) -> Model {
        Model {
            id,
            object: "model",
            created: self.started,
            owned_by: "veil402",
        }
    }
}

async fn models(State(api): State<Api>, _: Caller) -> Result<Json<Listed>, Refusal> {
    let data = api
        .envelopes
        .offers()
        .await?
        .into_iter()
        .map(|offer| api.listing(offer.model))
        .collect();
    Ok(Json(Listed {
        object: "list",
        data,
    }))
}

async fn model(
    State(api): State<Api>,
    _: Caller,
    Path(model): Path<String>,
) -> Result<Json<Model>, Refusal> {
    api.envelopes
        .offer(&model)
        .await?
        .map(|offer| Json(api.listing(offer.model)))
        .ok_or_else(|| Refusal::unlisted(&model))
}

#[derive(Serialize)]
struct Health {
    leaves: u32,
}

async fn healthz(State(api): State<Api>) -> Json<Health> {
    Json(Health {
        leaves: api.index.leaves().await,
    })
}
