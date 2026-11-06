use alloy::primitives::B256;

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
