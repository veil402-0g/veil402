//! Shared wire types for the console's list endpoints.

use alloy::hex;
use serde::{Deserialize, Serialize};

use super::primitives::Response;
use crate::core::agent::Mark;

/// A page of a collection. `next_cursor` is absent on the last one.
#[derive(Debug, Serialize)]
pub struct List<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> List<T> {
    pub const fn page(items: Vec<T>, next_cursor: Option<String>) -> Self {
        Self { items, next_cursor }
    }

    pub const fn all(items: Vec<T>) -> Self {
        Self {
            items,
            next_cursor: None,
        }
    }
}

/// `?limit=&cursor=`, the two parameters every listing takes.
#[derive(Debug, Default, Deserialize)]
pub struct Page {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

impl Page {
    const DEFAULT: u32 = 50;
    const MAX: u32 = 500;

    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(Self::DEFAULT).clamp(1, Self::MAX)
    }

    /// Where the last page stopped.
    ///
    /// # Errors
    ///
    /// Returns a `400` when the cursor was not one we issued.
    pub fn mark(&self) -> Result<Option<Mark>, Response<()>> {
        self.cursor
            .as_deref()
            .map(|cursor| {
                hex::decode(cursor)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .ok_or_else(|| Response::invalid("that cursor is not one we issued"))
            })
            .transpose()
    }
}

/// Hex rather than base64 only because hex is already in the tree. What
/// matters is that the client cannot read it and so cannot depend on it.
pub fn cursor(mark: &Mark) -> Option<String> {
    serde_json::to_vec(mark).ok().map(hex::encode)
}
