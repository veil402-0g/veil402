use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use veil402_sdk::{
    wire::{
        AcceptBody, AgreeBody, CallBody, CompletionBody, Offer, SessionBody, SettlementBody,
    },
};

use super::primitives::{Response, malformed};
use crate::core::{MeteringService, Serving};

/// Names the call a retry repeats, so one served twice is billed once.
const IDEMPOTENCY: &str = "idempotency-key";

pub fn router(service: Arc<MeteringService>) -> Router {
    Router::new()
        .route("/v1/catalogue", get(catalogue))
        .route("/v1/sessions/agree", post(agree))
        .route("/v1/sessions/accept", post(accept))
        .route("/v1/sessions/{nullifier}/chat", post(chat))
        .route("/v1/sessions/{nullifier}/settle", post(settle))
        .with_state(service)
}

async fn catalogue(State(service): State<Arc<MeteringService>>) -> Response<Vec<Offer>> {
    Response::ok(service.offers().to_vec())
}

async fn agree(
    State(service): State<Arc<MeteringService>>,
    Json(body): Json<SessionBody>,
) -> Response<AgreeBody> {
    let Ok(session) = body.parse() else {
        return malformed("nullifier");
    };
    service.agree(&session).map_or_else(Into::into, Response::ok)
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
    match service.meter(nullifier, &sealed, &attempt(&headers)).await {
        Ok(Serving::Metered(served)) => {
            Response::ok(CompletionBody::new(&served.completion, served.metered)).into_response()
        }
        Ok(Serving::Owing(challenge)) => Response::owing(challenge).into_response(),
        Err(error) => Response::<CompletionBody>::from(error).into_response(),
    }
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
