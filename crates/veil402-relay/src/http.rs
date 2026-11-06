//! What the daemon asks of the relay.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::relay::{Landed, Relay, RelayError};

#[derive(Deserialize)]
pub struct Submission {
    /// The caller's own key for this call — a nullifier or a commitment.
    key: String,
    calldata: String,
}

#[derive(Serialize)]
struct Refusal {
    error: String,
}

impl IntoResponse for RelayError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotRelayable => StatusCode::FORBIDDEN,
            Self::Refused(_) => StatusCode::BAD_GATEWAY,
        };
        (
            status,
            Json(Refusal {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

fn decoded(hex: &str) -> Result<Vec<u8>, RelayError> {
    let trimmed = hex.strip_prefix("0x").unwrap_or(hex);
    (0..trimmed.len())
        .step_by(2)
        .map(|i| {
            trimmed
                .get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| RelayError::Refused("not hex".into()))
        })
        .collect()
}

async fn submit(
    State(relay): State<Arc<Relay>>,
    Json(body): Json<Submission>,
) -> Result<StatusCode, RelayError> {
    let key = decoded(&body.key)?;
    relay.submit(&key, decoded(&body.calldata)?).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn landed(
    State(relay): State<Arc<Relay>>,
    Path(key): Path<String>,
) -> Result<Json<Landed>, RelayError> {
    Ok(Json(relay.landed(&decoded(&key)?).await?))
}

async fn healthz() -> StatusCode {
    StatusCode::OK
}

pub fn router(relay: Arc<Relay>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/submit", post(submit))
        .route("/v1/submit/{key}", get(landed))
        .with_state(relay)
}
