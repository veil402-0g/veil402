//! `GET /api/v1/activity` — the live feed, newest first.
//!
//! Three things happen in this system and all three belong on one timeline:
//! a deposit shields money, a lock reserves a note against a seller, and a
//! settled call draws on it. The audit endpoint answers only the third,
//! because only the third carries a proof.

use axum::extract::{Query, State};
use serde::Serialize;
use veil402_sdk::wire;

use super::{
    dto::{List, Page},
    primitives::{ApiResult, Operator, Response},
    state::Console,
};
use crate::core::agent::Audit;

/// What happened. The console colours and labels a row from this.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Deposit,
    Escrow,
    Settled,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub id: String,
    pub kind: Kind,
    pub at: u64,
    /// Who spent it. Absent for the two that are the daemon's own doing.
    pub agent: Option<String>,
    pub label: Option<String>,
    /// What it was spent on: a model for a call, the seller for a lock, the
    /// pool for a deposit.
    pub vendor: String,
    pub amount: u64,
    pub tokens: u64,
    /// The commitment or attestation, whichever identifies this row.
    pub commitment: Option<String>,
    pub nullifier: Option<String>,
}

pub async fn activity(
    State(console): State<Console>,
    _: Operator,
    Query(page): Query<Page>,
) -> ApiResult<List<Event>> {
    let limit = page.limit();
    let mut events = Vec::new();

    let filed = console
        .agents
        .audit(&Audit {
            limit: Some(limit),
            ..Audit::default()
        })
        .await?;
    for call in filed {
        let charge = &call.charge;
        events.push(Event {
            id: wire::encode(charge.attestation.as_slice()),
            kind: Kind::Settled,
            at: charge.at,
            agent: Some(call.agent.clone()),
            label: Some(call.label.clone()),
            vendor: charge.model.clone(),
            amount: charge.cost,
            tokens: charge.witness.as_ref().map_or(0, |witness| {
                witness.prompt.saturating_add(witness.completion)
            }),
            commitment: Some(wire::encode(charge.attestation.as_slice())),
            nullifier: charge.nullifier.map(|key| wire::encode(&key.to_bytes())),
        });
    }

    // A lock records a deadline rather than the second it landed, so the
    // window it was taken for is what dates it. The block number that did
    // carry it is long outside what the chain will answer for.
    for lock in console.index.escrows(limit).await? {
        events.push(Event {
            id: wire::encode(&lock.nullifier.to_bytes()),
            kind: Kind::Escrow,
            at: lock.deadline.saturating_sub(console.settings.window),
            agent: None,
            label: None,
            vendor: lock.seller.to_string(),
            amount: 0,
            tokens: 0,
            commitment: None,
            nullifier: Some(wire::encode(&lock.nullifier.to_bytes())),
        });
    }

    if let Some(deposit) = console.wallet.last_deposit().await? {
        events.push(Event {
            id: wire::encode(&deposit.commitment.to_bytes()),
            kind: Kind::Deposit,
            at: deposit.at,
            agent: None,
            label: None,
            vendor: "shielded pool".to_owned(),
            amount: deposit.amount,
            tokens: 0,
            commitment: Some(wire::encode(&deposit.commitment.to_bytes())),
            nullifier: None,
        });
    }

    events.sort_by_key(|event| std::cmp::Reverse(event.at));
    events.truncate(limit as usize);
    Ok(Response::ok(List::all(events)))
}
