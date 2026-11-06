use alloy::primitives::B256;
use serde::{Deserialize, Serialize};
use veil402_sdk::{Field, wire::Witness};

/// An agent, as the ledger knows it. One key, one row, one running total.
///
/// The enclave meters one envelope and never learns who asked; this is the
/// other half of that — the daemon knows who asked and files what the
/// enclave says it cost.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
    /// The public half of its key, and the only name it has here.
    pub id: String,
    /// What the key is for, so a list is readable.
    pub label: String,
    /// Pool units it may spend in total, or uncapped.
    pub budget: Option<u64>,
    pub spent: u64,
    pub calls: u64,
    pub created: u64,
    /// When it was last billed for anything.
    pub used: Option<u64>,
    pub revoked: bool,
    /// Stopped, but not for good. Revoking is the one-way door; this is the
    /// one an operator can walk back.
    pub paused: bool,
    /// Models this key may buy. `None` is any, which is what a key issued
    /// before the allowlist existed has.
    pub models: Option<Vec<String>>,
}

impl Agent {
    /// Whether this key may buy `model`.
    #[must_use]
    pub fn permits(&self, model: &str) -> bool {
        self.models
            .as_ref()
            .is_none_or(|allowed| allowed.iter().any(|listed| listed == model))
    }
}

/// An agent, and what proves a presented key is its.
pub struct Registered {
    pub agent: Agent,
    /// Hash of the secret half. The secret itself was never stored, so a
    /// copy of this table lets nobody spend anything.
    pub digest: B256,
}

/// A freshly issued key: the one moment it exists in full.
pub struct Issued {
    pub agent: Agent,
    /// Print it, hand it over, forget it. Nothing can recover it after this.
    pub key: String,
}

/// One served call, with everything proving it later needs.
///
/// The digest alone would make a ledger that balances and proves nothing:
/// what it was computed over has to be kept beside it, or a buyer holds a
/// number it cannot reproduce.
#[derive(Clone, Debug)]
pub struct Charge {
    pub attestation: B256,
    pub cost: u64,
    pub at: u64,
    /// The envelope it was served under, which is what ties it to a
    /// settlement on chain.
    pub nullifier: Field,
    pub delivered: Option<B256>,
    pub witness: Option<Witness>,
    /// Its index in the list that settlement roots.
    pub sequence: u64,
    pub model: String,
}

/// Where a page of an audit stopped.
///
/// Both halves, because the ledger is ordered by both: several calls served
/// in the same second are told apart only by their place in the envelope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Mark {
    pub at: u64,
    pub sequence: u64,
}

/// Which calls an audit covers.
///
/// Narrowing is the point: an auditor is shown one payment, one agent's
/// history, or one period, and nothing beside it.
#[derive(Clone, Debug, Default)]
pub struct Audit {
    pub agent: Option<String>,
    /// Inclusive bounds, in seconds.
    pub from: Option<u64>,
    pub to: Option<u64>,
    /// One call, by the digest that settles it.
    pub attestation: Option<B256>,
    /// Every call served under one envelope, which is how the open one is
    /// broken down per agent.
    pub nullifier: Option<Field>,
    /// Where the previous page stopped. Keyset rather than an offset: the
    /// ledger grows at the head while a reader is paging through it.
    pub after: Option<Mark>,
    /// Rows at most. `None` is every row the filters leave.
    pub limit: Option<u32>,
}

impl Filed {
    /// Where a page that ended on this row should resume.
    #[must_use]
    pub const fn mark(&self) -> Mark {
        Mark {
            at: self.charge.at,
            sequence: self.charge.sequence,
        }
    }
}

/// One call as an audit sees it: who made it, and what proves it.
#[derive(Clone, Debug)]
pub struct Filed {
    /// The key's id, which is what a filter names: labels are not unique.
    pub agent: String,
    /// What the key is for, so a page reads as something.
    pub label: String,
    pub charge: Charge,
}
