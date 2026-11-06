use axum::{
    Json,
    http::{
        HeaderName, HeaderValue, StatusCode,
        header::{CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::core::{
    agent::AgentError,
    envelope::{EnclaveError, EnvelopeError},
    wallet::WalletError,
};

/// `OpenAI`'s two broad kinds: one the caller can fix, one it cannot.
const INVALID: &str = "invalid_request_error";
const API: &str = "api_error";

/// How long to wait on something that might clear on its own.
const SOON: u64 = 5;

/// Honoured by `OpenAI`'s own clients, which otherwise back off and repeat a
/// 429 that repeating cannot change.
const SHOULD_RETRY: &str = "x-should-retry";

/// Statuses on which an upstream is talking about the request rather than
/// about the seller. Only these reach the agent in the upstream's own words:
/// the rest are our account, our provider, or our configuration, and saying
/// so in the upstream's voice would send an agent to fix the wrong thing.
const PASSED: [u16; 5] = [400, 404, 408, 413, 422];

/// A refusal in the shape every `OpenAI` client already parses.
pub struct Refusal {
    status: StatusCode,
    said: Said,
    /// Seconds to wait, when waiting is what helps.
    after: Option<u64>,
}

/// Whose words the agent is reading.
enum Said {
    Ours(Detail),
    /// The upstream's own, byte for byte. It already speaks the shape an
    /// agent parses, and rewriting it would lose the code a client decides
    /// on — `context_length_exceeded` is the difference between an agent
    /// shrinking its context and an agent retrying for ever.
    Theirs(Vec<u8>),
}

#[derive(Serialize)]
struct Detail {
    message: String,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

#[derive(Serialize)]
struct Fault {
    error: Detail,
}

impl Refusal {
    fn at(
        status: StatusCode,
        kind: &'static str,
        code: Option<&'static str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            said: Said::Ours(Detail {
                message: message.into(),
                kind,
                code,
            }),
            after: None,
        }
    }

    /// What the seller's upstream said, when it said something the agent can
    /// act on.
    ///
    /// A refusal about the seller's own account or provider is not one: an
    /// agent told `insufficient_balance` would send its user to top up the
    /// wrong account.
    fn upstream(status: u16, retry_after: Option<u64>, body: Vec<u8>) -> Self {
        let rated = status == StatusCode::TOO_MANY_REQUESTS.as_u16();
        if !(rated || PASSED.contains(&status)) || !speaks(&body) {
            let mut refusal = Self::at(
                StatusCode::SERVICE_UNAVAILABLE,
                API,
                Some("upstream_unavailable"),
                format!("the seller could not buy this call: its upstream answered {status}"),
            );
            refusal.after = Some(SOON);
            return refusal;
        }
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            said: Said::Theirs(body),
            // Its own number rather than ours: a rate limit we guess at is
            // one we hit again.
            after: rated.then(|| retry_after.unwrap_or(SOON)),
        }
    }

    /// Unissued, mistyped, revoked, or absent. One answer for all four,
    /// which is both what `OpenAI` does and the only safe thing to say.
    #[must_use]
    pub fn unauthenticated() -> Self {
        Self::at(
            StatusCode::UNAUTHORIZED,
            INVALID,
            Some("invalid_api_key"),
            "no valid API key was provided",
        )
    }

    #[must_use]
    pub fn malformed(message: impl Into<String>) -> Self {
        Self::at(StatusCode::BAD_REQUEST, INVALID, None, message)
    }

    #[must_use]
    pub fn unlisted(model: &str) -> Self {
        Self::at(
            StatusCode::NOT_FOUND,
            INVALID,
            Some("model_not_found"),
            format!("`{model}` is not a model this daemon can buy privately"),
        )
    }

    /// Something on our side that waiting may fix.
    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        let mut refusal = Self::at(
            StatusCode::SERVICE_UNAVAILABLE,
            API,
            Some("envelope_unavailable"),
            message,
        );
        refusal.after = Some(SOON);
        refusal
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::at(StatusCode::INTERNAL_SERVER_ERROR, API, None, message)
    }
}

impl From<AgentError> for Refusal {
    fn from(error: AgentError) -> Self {
        match error {
            AgentError::Unknown => Self::unauthenticated(),
            // `OpenAI` spends 429 on both a rate limit and a spent budget,
            // and tells them apart by the type rather than the status.
            AgentError::Exhausted { budget, committed } => Self::at(
                StatusCode::TOO_MANY_REQUESTS,
                "insufficient_quota",
                Some("budget_exhausted"),
                format!("this key has committed {committed} of its {budget}"),
            ),
            // The model exists and the seller sells it; this mandate just
            // does not cover it. `OpenAI` has no code for that, and the
            // nearest one would send an agent looking for a typo.
            AgentError::Unlisted { model } => Self::at(
                StatusCode::FORBIDDEN,
                INVALID,
                Some("model_not_permitted"),
                format!("this key's mandate does not cover `{model}`"),
            ),
            AgentError::Random | AgentError::Store(_) => {
                tracing::error!(%error, "the ledger would not answer");
                Self::internal("the ledger is unavailable")
            }
        }
    }
}

impl From<EnvelopeError> for Refusal {
    fn from(error: EnvelopeError) -> Self {
        tracing::warn!(%error, "a call was refused");
        let said = error.to_string();
        match error {
            // Nothing is left to buy with, and only an operator can change
            // that. Waiting is still the least wrong thing to ask.
            EnvelopeError::Wallet(WalletError::Empty) => {
                Self::unavailable("no note is held to open an envelope against")
            }
            EnvelopeError::Enclave(EnclaveError::Upstream {
                status,
                retry_after,
                body,
            }) => Self::upstream(status, retry_after, body),
            // The seller refused in words of its own, which are about the
            // seller rather than about the request.
            EnvelopeError::Enclave(EnclaveError::Refused(_)) => {
                Self::at(StatusCode::BAD_GATEWAY, API, None, said)
            }
            other if other.recoverable() => Self::unavailable(said),
            _ => Self::internal(said),
        }
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let Self {
            status,
            said,
            after,
        } = self;
        let mut response = match said {
            Said::Ours(detail) => (status, Json(Fault { error: detail })).into_response(),
            Said::Theirs(body) => (
                status,
                [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                body,
            )
                .into_response(),
        };
        let headers = response.headers_mut();
        if status == StatusCode::UNAUTHORIZED {
            headers.insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        match after {
            Some(after) => {
                headers.insert(RETRY_AFTER, HeaderValue::from(after));
            }
            None if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                headers.insert(
                    HeaderName::from_static(SHOULD_RETRY),
                    HeaderValue::from_static("false"),
                );
            }
            None => {}
        }
        response
    }
}

/// Whether an upstream answered in the shape a client parses. One that did
/// not would reach the agent as a JSON content type over something else.
fn speaks(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .is_ok_and(|said| said.get("error").is_some())
}
