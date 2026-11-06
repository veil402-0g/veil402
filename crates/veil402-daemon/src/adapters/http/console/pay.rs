//! `POST /api/v1/pay` — send a private payment to another wallet's address.
//!
//! Synchronous: it waits for the transfer to land, because the caller — an
//! agent settling a bill, or an operator at the console — has nothing to do
//! until it has.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use veil402_sdk::{session::PaymentAddress, wire};

use super::{
    primitives::{ApiResult, Response},
    state::Console,
};
use crate::app::now;

#[derive(Debug, Deserialize)]
pub struct Asked {
    pub address: String,
    pub amount: u64,
}

#[derive(Debug, Serialize)]
pub struct Sent {
    pub recipient_commitment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
}

pub async fn pay(State(console): State<Console>, Json(asked): Json<Asked>) -> ApiResult<Sent> {
    let to = asked
        .address
        .parse::<PaymentAddress>()
        .map_err(|_| Response::<()>::invalid("that is not a payable address"))?;
    let sent = console.payment.pay(to, asked.amount, now()).await?;
    Ok(Response::ok(Sent {
        recipient_commitment: wire::encode(&sent.recipient_commitment.to_bytes()),
        change: sent.change.map(|change| wire::encode(&change.to_bytes())),
    }))
}
