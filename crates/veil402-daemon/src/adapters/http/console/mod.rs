//! The operator surface: everything the console reads and changes.
//!
//! Kept apart from the agent API because the two answer to different people.
//! `/v1` is `OpenAI`'s namespace, authenticated by an agent's own key and
//! shaped so any `OpenAI` client parses it. This one reads across every
//! agent and wears an envelope of its own.
//!
//! It is open. The daemon binds to loopback, and that is the whole of what
//! keeps this surface to the machine it runs on.

mod activity;
mod address;
mod agents;
mod audit;
mod config;
mod confirm;
mod deposit;
mod dto;
mod error;
mod envelope;
mod pay;
mod pool;
mod primitives;
mod state;

use axum::{
    Router,
    routing::{delete, get, post},
};

pub use state::{Console, Settings};

pub fn router(console: Console) -> Router {
    Router::new()
        .route("/config", get(config::config))
        .route("/pool", get(pool::pool))
        .route("/address", get(address::address))
        .route("/pay", post(pay::pay))
        .route("/confirm", post(confirm::confirm))
        .route("/deposit", get(deposit::progress).post(deposit::start))
        .route("/envelope", get(envelope::envelope))
        .route("/activity", get(activity::activity))
        .route("/audit", get(audit::audit))
        .route("/agents", get(agents::agents).post(agents::issue))
        .route("/agents/{id}", delete(agents::revoke))
        .route("/agents/{id}/pause", post(agents::pause))
        .route("/agents/{id}/resume", post(agents::resume))
        .with_state(console)
}
