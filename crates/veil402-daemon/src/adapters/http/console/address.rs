//! `GET /api/v1/address` — where this wallet can be privately paid.

use axum::extract::State;
use serde::Serialize;

use super::{
    primitives::{ApiResult, Response},
    state::Console,
};

#[derive(Debug, Serialize)]
pub struct Address {
    pub address: String,
}

pub async fn address(State(console): State<Console>) -> ApiResult<Address> {
    let address = console.wallet.address()?;
    Ok(Response::ok(Address {
        address: address.to_string(),
    }))
}
