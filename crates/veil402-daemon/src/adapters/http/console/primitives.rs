//! The envelope every console response wears, and the credential that opens
//! the surface.
//!
//! Separate from the agent API's [`Refusal`](crate::adapters::http::Refusal)
//! on purpose: that one answers in `OpenAI`'s shapes because `OpenAI`'s
//! clients parse it. Nothing here has to.

use axum::{
    Json, RequestPartsExt,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response as AxumResponse},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use serde::Serialize;
use subtle::ConstantTimeEq;

use super::state::Console;

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

/// The operator, who sees every agent's ledger.
///
/// An extractor rather than a line in each handler, for the same reason the
/// agent API uses one: a console route that forgets to authenticate does not
/// compile. It is deliberately not an agent key — an agent authenticates as
/// itself, and this surface reads across all of them.
pub struct Operator;

impl FromRequestParts<Console> for Operator {
    type Rejection = Response<()>;

    async fn from_request_parts(
        parts: &mut Parts,
        console: &Console,
    ) -> Result<Self, Self::Rejection> {
        // No token configured is a closed door, not an open one: an empty
        // secret would otherwise be matched by an empty header.
        let Some(expected) = console.token.as_deref() else {
            return Err(Response::error(
                "the console is not enabled on this daemon",
                StatusCode::SERVICE_UNAVAILABLE,
            ));
        };
        let bearer = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| unauthenticated())?;
        bool::from(bearer.token().as_bytes().ct_eq(expected.as_bytes()))
            .then_some(Self)
            .ok_or_else(unauthenticated)
    }
}

fn unauthenticated() -> Response<()> {
    Response::error(
        "no valid operator token was provided",
        StatusCode::UNAUTHORIZED,
    )
}
