use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response as AxumResponse},
};
use serde::Serialize;
use veil402_sdk::wire::{Action, Reply};

pub use veil402_sdk::wire::Status;

use crate::core::MeterError;

/// A reply and the status that carries it.
///
/// The envelope is the wire contract and lives in the SDK, so the daemon
/// reads the same type it is sent; the status is this side's only.
pub struct Response<T> {
    reply: Reply<T>,
    status: StatusCode,
}

impl<T> Response<T> {
    pub const fn ok(result: T) -> Self {
        Self {
            reply: Reply::ok(result),
            status: StatusCode::OK,
        }
    }

    pub fn failed(error: &impl ToString, status: StatusCode, action: Action) -> Self {
        Self {
            reply: Reply::failed(error, action),
            status,
        }
    }

    /// A buyer with no envelope is told what one would take, not that
    /// something went wrong.
    pub fn owing(challenge: T) -> Self {
        Self {
            reply: Reply::owing(challenge, &"no envelope covers this call"),
            status: StatusCode::PAYMENT_REQUIRED,
        }
    }
}

impl<T: Serialize> IntoResponse for Response<T> {
    fn into_response(self) -> AxumResponse {
        let status = self.status;
        let mut response = Json(self.reply).into_response();
        *response.status_mut() = status;
        response
    }
}

/// What the daemon should do, and under which status.
///
/// The action is the point: a status says what went wrong, and the only
/// question the daemon has is what to do next.
const fn verdict(error: &MeterError) -> (StatusCode, Action) {
    match error {
        // The enclave restarted and lost its sessions. The delegation is
        // still valid and the escrow still stands, so handing it over again
        // is the whole repair.
        MeterError::Unknown => (StatusCode::NOT_FOUND, Action::Retry),
        MeterError::CeilingReached | MeterError::Empty => {
            (StatusCode::CONFLICT, Action::Reshop)
        }
        MeterError::Refused(_) => (StatusCode::UNPROCESSABLE_ENTITY, Action::Fail),
    }
}

impl<T> From<MeterError> for Response<T> {
    fn from(error: MeterError) -> Self {
        let (status, action) = verdict(&error);
        tracing::warn!(%error, "request refused");
        Self::failed(&error, status, action)
    }
}

/// A caller that sent something unreadable. Retrying it cannot help.
pub fn malformed<T>(what: &'static str) -> Response<T> {
    Response::failed(
        &format!("{what} is malformed"),
        StatusCode::BAD_REQUEST,
        Action::Fail,
    )
}
