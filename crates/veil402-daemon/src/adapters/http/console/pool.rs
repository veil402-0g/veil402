//! `GET /api/v1/pool` — what this daemon holds, and how current it is.
//!
//! `leaves` is every commitment the pool has ever emitted, which is the
//! anonymity set a spend hides in; `notes` is only ours. Reporting both is
//! the difference between "we hold 39 notes" and "those 39 hide among
//! 12,408".

use axum::extract::State;
use serde::Serialize;
use veil402_sdk::wire;

use super::{
    primitives::{ApiResult, Response},
    state::Console,
};

#[derive(Debug, Serialize)]
pub struct Pool {
    /// In pool units. `decimals` from `/config` turns it into money.
    pub balance: u64,
    /// Notes we own and can spend.
    pub notes: u32,
    /// Commitments in the whole tree.
    pub leaves: u32,
    pub root: String,
    /// Whether the local tree agrees with the pool's own root. Being behind
    /// is ordinary; being wrong is not.
    pub synced: bool,
    /// What the funding key still holds in the clear, so a console can offer
    /// to shield all of it. Absent when the chain would not say.
    pub depositable: Option<u64>,
    pub last_deposit: Option<Deposited>,
}

#[derive(Debug, Serialize)]
pub struct Deposited {
    pub amount: u64,
    pub at: u64,
}

pub async fn pool(State(console): State<Console>) -> ApiResult<Pool> {
    let holdings = console.wallet.holdings().await?;
    // The local tree, not `health`: that one asks the pool whether it ever
    // held this root, and a chain that is away would fail the whole page.
    let root = console.index.root().await?;
    Ok(Response::ok(Pool {
        balance: holdings.balance,
        notes: holdings.notes,
        leaves: console.index.leaves().await,
        root: wire::encode(&root.to_bytes()),
        synced: console.index.is_current().await.unwrap_or(false),
        depositable: console.funding.depositable().await.ok(),
        last_deposit: console
            .wallet
            .last_deposit()
            .await?
            .map(|deposit| Deposited {
                amount: deposit.amount,
                at: deposit.at,
            }),
    }))
}
