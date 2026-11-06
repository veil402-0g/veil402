//! `POST /api/v1/deposit` shields funds; `GET` says how far it has got.
//!
//! Two calls rather than one, because shielding takes an allowance, a
//! transfer and an inclusion — the better part of a minute — and a request
//! held open for that is one a browser gives up on.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use super::{
    primitives::{ApiResult, Response},
    state::Console,
};
use crate::{
    app::now,
    core::treasury::{Progress, TreasuryError},
};

#[derive(Debug, Deserialize)]
pub struct Asked {
    /// Pool units to shield.
    pub amount: u64,
}

#[derive(Debug, Serialize)]
pub struct Started {
    pub accepted: bool,
}

pub async fn start(
    State(console): State<Console>,
    Json(asked): Json<Asked>,
) -> ApiResult<Started> {
    let treasury = Arc::clone(&console.treasury);
    // Answered before it is finished: the work outlives the request, and
    // what it is doing is read back from the other route.
    treasury
        .begin(asked.amount, now())
        .map_err(Response::<()>::from)?;
    tokio::spawn(async move {
        if let Err(error) = treasury.fund(asked.amount, now()).await {
            tracing::warn!(%error, "a deposit did not land");
        }
    });
    Ok(Response::ok(Started { accepted: true }))
}

pub async fn progress(State(console): State<Console>) -> ApiResult<Option<Progress>> {
    Ok(Response::ok(console.treasury.progress()))
}

impl From<TreasuryError> for Response<()> {
    fn from(error: TreasuryError) -> Self {
        match error {
            TreasuryError::Busy | TreasuryError::Nothing => Self::invalid(error.to_string()),
            _ => {
                tracing::error!(%error, "a deposit failed");
                Self::error(error.to_string(), axum::http::StatusCode::BAD_GATEWAY)
            }
        }
    }
}
