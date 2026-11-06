//! `GET /api/v1/config` — what the console reads before it draws anything.
//!
//! Addresses and limits come from the daemon's own configuration; `models`
//! comes from the seller's catalogue, so the mandate form offers what can
//! actually be bought rather than a list compiled into the page.

use axum::extract::State;
use serde::Serialize;

use super::{
    primitives::{ApiResult, Operator, Response},
    state::{Console, Settings},
};

#[derive(Debug, Serialize)]
pub struct Bootstrap {
    #[serde(flatten)]
    pub settings: Settings,
    pub models: Vec<String>,
}

pub async fn config(State(console): State<Console>, _: Operator) -> ApiResult<Bootstrap> {
    // A seller that is away leaves the console usable: every panel but the
    // mandate form's chips works without knowing the catalogue.
    let models = console.envelopes.offers().await.map_or_else(
        |error| {
            tracing::warn!(%error, "the catalogue could not be read");
            Vec::new()
        },
        |offers| offers.into_iter().map(|offer| offer.model).collect(),
    );
    Ok(Response::ok(Bootstrap {
        settings: (*console.settings).clone(),
        models,
    }))
}
