//! The envelope every console response wears.
//!
//! Separate from the agent API's [`Refusal`](crate::adapters::http::Refusal)
//! on purpose: that one answers in `OpenAI`'s shapes because `OpenAI`'s
//! clients parse it. Nothing here has to.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response as AxumResponse},
};
use serde::Serialize;

/// Whether the call succeeded. The body says so as well as the status, so a
/// client that only reads JSON still knows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Error,
}

/// What every console route returns, successful or not.
#[derive(Debug, Serialize)]
pub struct Response<T> {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip)]
    pub code: StatusCode,
}

impl<T> Response<T> {
    pub const fn ok(result: T) -> Self {
        Self {
            status: Status::Ok,
            result: Some(result),
            error: None,
            code: StatusCode::OK,
        }
    }

    pub fn error(error: impl Into<String>, code: StatusCode) -> Self {
        Self {
            status: Status::Error,
            result: None,
            error: Some(error.into()),
            code,
        }
    }

    /// Something the caller got wrong and can correct.
    pub fn invalid(error: impl Into<String>) -> Self {
        Self::error(error, StatusCode::BAD_REQUEST)
    }

    pub fn missing(error: impl Into<String>) -> Self {
        Self::error(error, StatusCode::NOT_FOUND)
    }
}

impl<T: Serialize> IntoResponse for Response<T> {
    fn into_response(self) -> AxumResponse {
        let code = self.code;
        let mut response = Json(self).into_response();
        *response.status_mut() = code;
        response
    }
}

/// What a console handler returns. Both arms are the same wire shape.
pub type ApiResult<T> = Result<Response<T>, Response<()>>;
