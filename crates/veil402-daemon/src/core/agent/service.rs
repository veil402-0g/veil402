use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use alloy::{
    hex,
    primitives::{B256, keccak256},
};
use subtle::ConstantTimeEq;
use thiserror::Error;

use super::{Agent, AgentStore, Issued};
use crate::core::StoreError;

/// What an agent presents.
///
/// `sk-` so anything sniffing for an `OpenAI` key is satisfied, `veil` so a
/// secret scanner can be given a rule, and an id so checking one is a single
/// indexed read rather than a scan of every key ever issued.
const PREFIX: &str = "sk-veil-";

/// Bytes in each half. The id is a handle and public; the secret is the
/// whole of the authentication, so it is full width rather than something a
/// person could be asked to type.
const ID: usize = 8;
const SECRET: usize = 32;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AgentError {
    /// Unissued, mistyped, or revoked. One answer for all three: which it
    /// was is only ever useful to somebody guessing.
    #[error("that key is not one of ours")]
    Unknown,
    #[error("this agent has committed {committed} of its {budget}")]
    Exhausted { budget: u64, committed: u64 },
    /// The machine would not produce randomness, which is not a condition
    /// anything here can work around.
    #[error("no randomness is available")]
    Random,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Headroom reserved for a call in flight.
///
/// Dropping it is the release: an early return, a client that hangs up, a
/// panic — each gives the headroom back, which a release that has to be
/// called would not.
pub struct Hold {
    agent: String,
    amount: u64,
    held: Arc<Mutex<HashMap<String, u64>>>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        if let Ok(mut held) = self.held.lock()
            && let Some(flight) = held.get_mut(&self.agent)
        {
            *flight = flight.saturating_sub(self.amount);
        }
    }
}

/// Issues the keys agents authenticate with, and keeps their books.
pub struct AgentService {
    agents: Arc<dyn AgentStore>,
    /// What calls in flight have reserved, by agent. Not durable on
    /// purpose: a call does not survive a restart, so neither should its
    /// reservation.
    held: Arc<Mutex<HashMap<String, u64>>>,
}

impl AgentService {
    #[must_use]
    pub fn new(agents: Arc<dyn AgentStore>) -> Self {
        Self {
            agents,
            held: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Issues a key, returning the plaintext once.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError`] when the machine has no randomness, or the
    /// store refuses the write.
    pub async fn issue(
        &self,
        label: &str,
        budget: Option<u64>,
        now: u64,
    ) -> Result<Issued, AgentError> {
        let secret = random::<SECRET>()?;
        let agent = Agent {
            id: hex::encode(random::<ID>()?),
            label: label.to_owned(),
            budget,
            spent: 0,
            calls: 0,
            created: now,
            used: None,
            revoked: false,
        };
        self.agents.issue(&agent, keccak256(secret)).await?;
        let key = format!("{PREFIX}{}-{}", agent.id, hex::encode(secret));
        Ok(Issued { agent, key })
    }

    /// The agent a presented key belongs to.
    ///
    /// The id is the lookup and the secret is the proof. Only a hash of the
    /// secret was kept, and it is compared in constant time, so a near miss
    /// takes exactly as long as a wild one.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Unknown`] for anything that is not a live key,
    /// whichever way it fails.
    pub async fn authenticate(&self, presented: &str) -> Result<Agent, AgentError> {
        let (id, secret) = split(presented).ok_or(AgentError::Unknown)?;
        let registered = self
            .agents
            .registered(&id)
            .await?
            .ok_or(AgentError::Unknown)?;
        let proven = keccak256(secret).0.ct_eq(&registered.digest.0);
        if registered.agent.revoked || !bool::from(proven) {
            return Err(AgentError::Unknown);
        }
        Ok(registered.agent)
    }

    /// Reserves headroom for one call.
    ///
    /// Against the snapshot the call authenticated with, so a charge filed
    /// in between is one call's worth of slack rather than a race worth
    /// locking for. The envelope's ceiling is the hard stop either way, and
    /// the enclave holds that.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Exhausted`] when the estimate would take the
    /// agent past its budget, counting what it already has in flight.
    pub fn reserve(&self, agent: &Agent, estimate: u64) -> Result<Hold, AgentError> {
        let mut held = self.held.lock().map_err(|_| StoreError::Unavailable)?;
        let flight = held.entry(agent.id.clone()).or_insert(0);
        let committed = agent.spent.saturating_add(*flight);
        if let Some(budget) = agent.budget
            && committed.saturating_add(estimate) > budget
        {
            return Err(AgentError::Exhausted { budget, committed });
        }
        *flight = flight.saturating_add(estimate);
        Ok(Hold {
            agent: agent.id.clone(),
            amount: estimate,
            held: self.held.clone(),
        })
    }

    /// Files what a call actually cost, and gives its reservation back.
    ///
    /// The bill rather than the estimate: an upstream prices work once it
    /// has done it, and the estimate was only ever an authorisation.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Store`] when the ledger cannot be written.
    pub async fn charge(
        &self,
        hold: Hold,
        attestation: B256,
        cost: u64,
        now: u64,
    ) -> Result<(), AgentError> {
        // Filed before the hold is dropped, so the window between the two
        // counts the call twice rather than not at all.
        Ok(self
            .agents
            .charge(attestation, &hold.agent, cost, now)
            .await?)
    }

    /// Every agent, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Store`] when the ledger cannot be read.
    pub async fn listed(&self) -> Result<Vec<Agent>, AgentError> {
        Ok(self.agents.listed().await?)
    }

    /// Stops a key authenticating. Whether it was a key at all.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::Store`] when the ledger cannot be written.
    pub async fn revoke(&self, id: &str) -> Result<bool, AgentError> {
        Ok(self.agents.revoke(id).await?)
    }
}

/// Takes a presented key apart.
///
/// Nothing here depends on a secret, so a key that is not even shaped like
/// one never reaches the store.
fn split(presented: &str) -> Option<(String, [u8; SECRET])> {
    let (id, secret) = presented.trim().strip_prefix(PREFIX)?.split_once('-')?;
    let secret: [u8; SECRET] = hex::decode(secret).ok()?.try_into().ok()?;
    (id.len() == ID * 2).then(|| (id.to_owned(), secret))
}

fn random<const N: usize>() -> Result<[u8; N], AgentError> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|_| AgentError::Random)?;
    Ok(bytes)
}
