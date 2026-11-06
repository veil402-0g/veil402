//! Two surfaces on one port.
//!
//! `/v1` is the agent API: `OpenAI`'s shape over a shielded note, so any
//! client that speaks it works unchanged. `/api/v1` is the operator console,
//! which reads across every agent and so cannot be authenticated by an
//! agent's own key.

mod agent;
mod console;
mod primitives;

use std::time::Duration;

use axum::{
    Router,
    http::{HeaderValue, StatusCode},
};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

pub use agent::Api;
pub use console::{Console, Settings};
pub use primitives::Refusal;

/// Long enough for a model that thinks before it answers; short enough that
/// a wedged upstream does not hold a connection for ever. The relay sends
/// keep-alives, so a slow answer is not an idle one.
const PATIENCE: Duration = Duration::from_mins(10);

/// Where the console is served from, when it is not this origin.
///
/// Browsers refuse a cross-origin read without this, and the console is a
/// separate dev server. An allowlist rather than `permissive`: this surface
/// issues mandates.
pub fn router(api: Api, console: Console, origins: &[HeaderValue]) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins.iter().cloned()))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);
    agent::router(api)
        .nest("/api/v1", console::router(console))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::GATEWAY_TIMEOUT,
                    PATIENCE,
                ))
                .layer(cors),
        )
}
