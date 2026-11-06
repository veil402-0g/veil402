//! The mandates: who may spend, how much, and on what.
//!
//! `GET /api/v1/agents` lists them, `POST` issues one, `DELETE` revokes it,
//! and `POST .../pause` holds it without closing the one-way door.

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{
    dto::List,
    primitives::{ApiResult, Response},
    state::Console,
};
use crate::{app::now, core::agent::Agent};

#[derive(Debug, Serialize)]
pub struct Mandate {
    pub id: String,
    pub label: String,
    /// Absent means uncapped — the envelope's ceiling is then the only stop.
    pub budget: Option<u64>,
    pub spent: u64,
    pub calls: u64,
    pub created: u64,
    /// When it was last billed for anything.
    pub used: Option<u64>,
    pub revoked: bool,
    pub paused: bool,
    /// Models it may buy. Absent means any.
    pub models: Option<Vec<String>>,
}

impl From<Agent> for Mandate {
    fn from(agent: Agent) -> Self {
        Self {
            id: agent.id,
            label: agent.label,
            budget: agent.budget,
            spent: agent.spent,
            calls: agent.calls,
            created: agent.created,
            used: agent.used,
            revoked: agent.revoked,
            paused: agent.paused,
            models: agent.models,
        }
    }
}

pub async fn agents(State(console): State<Console>) -> ApiResult<List<Mandate>> {
    let listed = console.agents.listed().await?;
    Ok(Response::ok(List::all(
        listed.into_iter().map(Mandate::from).collect(),
    )))
}

#[derive(Debug, Deserialize)]
pub struct Issue {
    pub label: String,
    pub budget: Option<u64>,
    #[serde(default)]
    pub models: Option<Vec<String>>,
}

/// A freshly issued key. The secret is here and nowhere else — it was never
/// stored, so this response is the only chance to keep it.
#[derive(Debug, Serialize)]
pub struct Granted {
    #[serde(flatten)]
    pub mandate: Mandate,
    pub key: String,
}

pub async fn issue(
    State(console): State<Console>,
    Json(asked): Json<Issue>,
) -> ApiResult<Granted> {
    let label = asked.label.trim();
    if label.is_empty() {
        return Err(Response::invalid("a mandate needs a label"));
    }
    let issued = console
        .agents
        .issue(label, asked.budget, asked.models, now())
        .await?;
    Ok(Response::ok(Granted {
        mandate: issued.agent.into(),
        key: issued.key,
    }))
}

/// Whether the call changed anything. A second revoke is not an error, but
/// it is worth telling apart from the first.
#[derive(Debug, Serialize)]
pub struct Changed {
    pub changed: bool,
}

pub async fn revoke(
    State(console): State<Console>,
    Path(id): Path<String>,
) -> ApiResult<Changed> {
    let changed = console.agents.revoke(&id).await?;
    Ok(Response::ok(Changed { changed }))
}

pub async fn pause(
    State(console): State<Console>,
    Path(id): Path<String>,
) -> ApiResult<Changed> {
    held(&console, &id, true).await
}

pub async fn resume(
    State(console): State<Console>,
    Path(id): Path<String>,
) -> ApiResult<Changed> {
    held(&console, &id, false).await
}

async fn held(console: &Console, id: &str, paused: bool) -> ApiResult<Changed> {
    let changed = console.agents.pause(id, paused).await?;
    Ok(Response::ok(Changed { changed }))
}
