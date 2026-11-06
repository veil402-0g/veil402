//! Domain errors as console responses.
//!
//! Only what the caller can act on carries its own words. Everything else is
//! an infrastructure failure: logged in full, answered generically, because a
//! store's own message is no use to a browser and may say more than it should.

use std::fmt::Display;

use axum::http::StatusCode;

use super::primitives::Response;
use crate::core::{
    agent::AgentError, envelope::EnvelopeError, index::IndexError, wallet::WalletError,
};

const INTERNAL: &str = "the daemon could not answer";

/// Logs the cause and keeps it out of the body.
fn internal(error: &dyn Display) -> Response<()> {
    tracing::error!(%error, "a console request failed");
    Response::error(INTERNAL, StatusCode::INTERNAL_SERVER_ERROR)
}

impl From<AgentError> for Response<()> {
    fn from(error: AgentError) -> Self {
        match error {
            // The one agent failure a console operator caused rather than
            // suffered: a mandate that does not cover what was asked for.
            AgentError::Unlisted { .. } | AgentError::Exhausted { .. } => {
                Self::invalid(error.to_string())
            }
            AgentError::Unknown => Self::missing("no such agent"),
            _ => internal(&error),
        }
    }
}

impl From<WalletError> for Response<()> {
    fn from(error: WalletError) -> Self {
        internal(&error)
    }
}

impl From<EnvelopeError> for Response<()> {
    fn from(error: EnvelopeError) -> Self {
        internal(&error)
    }
}

impl From<IndexError> for Response<()> {
    fn from(error: IndexError) -> Self {
        internal(&error)
    }
}
