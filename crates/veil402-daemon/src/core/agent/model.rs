use alloy::primitives::B256;
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
