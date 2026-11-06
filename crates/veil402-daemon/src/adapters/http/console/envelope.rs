//! `GET /api/v1/envelope` — the escrow serving calls right now.
//!
//! One envelope serves every agent: the pool holds a single lock against a
//! single note, and the ceiling belongs to that note rather than to whoever
//! spends against it. So the per-agent breakdown here is a view of one
//! shared ceiling, not several escrows.

use std::collections::BTreeMap;

use axum::extract::State;
use serde::Serialize;
use veil402_sdk::wire;

use super::{
    primitives::{ApiResult, Operator, Response},
    state::Console,
};
use crate::core::agent::{Audit, Filed};

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub nullifier: String,
    pub seller: String,
    /// The note's value: the most this envelope can ever cost.
    pub ceiling: u64,
    pub spent: u64,
    pub remaining: u64,
    pub calls: u64,
    pub tokens: u64,
    pub opened: u64,
    pub deadline: u64,
    /// Who has drawn on it, and for how much.
    pub agents: Vec<Drawn>,
}

/// One agent's draw on the shared envelope.
#[derive(Debug, Serialize)]
pub struct Drawn {
    pub id: String,
    pub label: String,
    /// What this agent has actually spent.
    pub metered: u64,
    /// That spend plus an equal share of the ceiling nobody has claimed
    /// yet, so the shares add up to the one real ceiling and a meter never
    /// reads past full. The split is presentational: the pool holds one
    /// lock, and any agent may spend the whole remainder.
    pub escrow: u64,
    pub calls: u64,
    pub tokens: u64,
    /// The model it bought most recently under this envelope.
    pub model: String,
}

pub async fn envelope(
    State(console): State<Console>,
    _: Operator,
) -> ApiResult<Option<Envelope>> {
    let Some(open) = console.envelopes.current().await? else {
        return Ok(Response::ok(None));
    };
    let filed = console
        .agents
        .audit(&Audit {
            nullifier: Some(open.nullifier),
            ..Audit::default()
        })
        .await?;

    let spent: u64 = filed.iter().map(|call| call.charge.cost).sum();
    let remaining = open.ceiling.saturating_sub(spent);
    let agents = drawn(&filed, remaining);
    Ok(Response::ok(Some(Envelope {
        nullifier: wire::encode(&open.nullifier.to_bytes()),
        seller: open.seller.to_string(),
        ceiling: open.ceiling,
        spent,
        remaining,
        calls: filed.len() as u64,
        tokens: filed.iter().map(tokens).sum(),
        opened: open.opened,
        deadline: open.deadline,
        agents,
    })))
}

/// Groups the envelope's calls by the agent that made them.
///
/// Newest first on the way in, so the first row seen for an agent carries
/// the model it bought most recently.
fn drawn(filed: &[Filed], remaining: u64) -> Vec<Drawn> {
    let mut by_agent: BTreeMap<&str, Drawn> = BTreeMap::new();
    for call in filed {
        let row = by_agent.entry(&call.agent).or_insert_with(|| Drawn {
            id: call.agent.clone(),
            label: call.label.clone(),
            metered: 0,
            escrow: 0,
            calls: 0,
            tokens: 0,
            model: call.charge.model.clone(),
        });
        row.metered = row.metered.saturating_add(call.charge.cost);
        row.calls += 1;
        row.tokens = row.tokens.saturating_add(tokens(call));
    }

    let mut rows: Vec<_> = by_agent.into_values().collect();
    let share = remaining
        .checked_div(rows.len() as u64)
        .unwrap_or_default();
    for row in &mut rows {
        row.escrow = row.metered.saturating_add(share);
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.metered));
    rows
}

/// What one call metered, as the upstream counted it. A call served before
/// the witness existed reports none rather than a wrong number.
fn tokens(call: &Filed) -> u64 {
    call.charge
        .witness
        .as_ref()
        .map_or(0, |witness| witness.prompt.saturating_add(witness.completion))
}
