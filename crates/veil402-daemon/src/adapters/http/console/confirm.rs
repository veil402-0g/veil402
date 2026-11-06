//! `POST /api/v1/confirm` — the payee verifying it was paid for a request.
//!
//! The payer presents the commitment of the note it sent and that note's
//! blinding. The blinding is never on chain — only the payer and this wallet
//! know it — so a note this wallet owns, of the agreed value, whose blinding
//! matches, proves the claimer is the one who paid. A chain-watcher who only
//! saw the commitment cannot produce the blinding.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use veil402_sdk::wire;

use super::{
    primitives::{ApiResult, Response},
    state::Console,
};

#[derive(Debug, Deserialize)]
pub struct Asked {
    pub commitment: String,
    pub blinding: String,
    pub amount: u64,
}

#[derive(Debug, Serialize)]
pub struct Confirmed {
    pub confirmed: bool,
}

pub async fn confirm(
    State(console): State<Console>,
    Json(asked): Json<Asked>,
) -> ApiResult<Confirmed> {
    let commitment = wire::field(&asked.commitment)
        .map_err(|_| Response::<()>::invalid("that is not a commitment"))?;
    let blinding = wire::bytes(&asked.blinding)
        .map_err(|_| Response::<()>::invalid("that is not a blinding"))?;
    let confirmed = match console.wallet.by_commitment(commitment).await? {
        Some(note) => {
            note.value == asked.amount && note.random.to_bytes().as_slice() == blinding.as_slice()
        }
        None => false,
    };
    Ok(Response::ok(Confirmed { confirmed }))
}
