//! `GET /api/v1/audit` — the calls an audit covers, each with what proves it.
//!
//! Read-only and narrowed: this is what an auditor is handed, so it carries
//! the binding and the settlement it belongs to, never the answer itself.
//! The feed on the console's front page is [`super::activity`]; this one is
//! the record you export.

use alloy::primitives::B256;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use veil402_sdk::{wire, wire::Witness};

use super::{
    dto::{List, Page, cursor},
    primitives::{ApiResult, Operator, Response},
    state::Console,
};
use crate::core::agent::{Audit, Filed};

/// What an audit narrows to. Everything absent means everything.
#[derive(Debug, Deserialize)]
pub struct Scope {
    pub agent: Option<String>,
    /// Inclusive bounds, in seconds.
    pub from: Option<u64>,
    pub to: Option<u64>,
    /// One call, by the digest that settles it.
    pub tx: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Call {
    /// The attestation, which is this call's only identity.
    pub id: String,
    pub agent: String,
    pub label: String,
    pub model: String,
    pub cost: u64,
    pub at: u64,
    pub tokens: Option<u64>,
    /// Enough to recompute the attestation and place it in the root the
    /// settlement signed; the answer stays sealed.
    pub proof: Proof,
}

#[derive(Debug, Serialize)]
pub struct Proof {
    /// The envelope it was served under, which ties it to a settlement.
    pub nullifier: String,
    pub attestation: String,
    pub delivered: Option<String>,
    /// Its index in the list that settlement roots.
    pub sequence: u64,
    pub witness: Option<Witness>,
}

pub async fn audit(
    State(console): State<Console>,
    _: Operator,
    Query(scope): Query<Scope>,
    Query(page): Query<Page>,
) -> ApiResult<List<Call>> {
    let attestation = scope.tx.as_deref().map(digest).transpose()?;
    let limit = page.limit();
    let filed = console
        .agents
        .audit(&Audit {
            agent: scope.agent,
            from: scope.from,
            to: scope.to,
            attestation,
            nullifier: None,
            after: page.mark()?,
            // One past the page, so a full page can be told from the last
            // one without a second count query.
            limit: Some(limit.saturating_add(1)),
        })
        .await?;

    let more = filed.len() > limit as usize;
    let shown = &filed[..filed.len().min(limit as usize)];
    let next = more.then(|| shown.last().map(Filed::mark)).flatten();
    Ok(Response::ok(List::page(
        shown.iter().map(call).collect(),
        next.as_ref().and_then(cursor),
    )))
}

fn call(filed: &Filed) -> Call {
    let charge = &filed.charge;
    Call {
        id: hexed(&charge.attestation),
        agent: filed.agent.clone(),
        label: filed.label.clone(),
        model: charge.model.clone(),
        cost: charge.cost,
        at: charge.at,
        tokens: charge
            .witness
            .as_ref()
            .map(|witness| witness.prompt.saturating_add(witness.completion)),
        proof: Proof {
            nullifier: wire::encode(&charge.nullifier.to_bytes()),
            attestation: hexed(&charge.attestation),
            delivered: charge.delivered.as_ref().map(hexed),
            sequence: charge.sequence,
            witness: charge.witness.clone(),
        },
    }
}

fn hexed(digest: &B256) -> String {
    wire::encode(digest.as_slice())
}

fn digest(tx: &str) -> Result<B256, Response<()>> {
    wire::bytes(tx)
        .ok()
        .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
        .map(B256::from)
        .ok_or_else(|| Response::invalid("tx is not a 32-byte digest"))
}
