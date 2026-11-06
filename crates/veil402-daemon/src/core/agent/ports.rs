use alloy::primitives::B256;
use async_trait::async_trait;

use super::{Agent, Registered};
use crate::core::StoreError;

/// Who may spend, and what they have spent.
#[async_trait]
pub trait AgentStore: Send + Sync {
    /// Writes a new agent down with the digest its key is checked against.
    async fn issue(&self, agent: &Agent, digest: B256) -> Result<(), StoreError>;

    /// The agent an id names, with what proves a key is its.
    async fn registered(&self, id: &str) -> Result<Option<Registered>, StoreError>;

    /// Every agent, newest first.
    async fn listed(&self) -> Result<Vec<Agent>, StoreError>;

    /// Stops a key authenticating. What it spent stays on the books.
    async fn revoke(&self, id: &str) -> Result<bool, StoreError>;

    /// Files one call's cost against the agent that made it.
    ///
    /// Idempotent on `attestation`, which is the enclave's own digest for
    /// the call: a retry, a restart, or a drain that outlived its reader
    /// files the same call again and is billed for it once.
    async fn charge(
        &self,
        attestation: B256,
        agent: &str,
        cost: u64,
        now: u64,
    ) -> Result<(), StoreError>;
}
