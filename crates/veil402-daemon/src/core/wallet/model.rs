use veil402_sdk::{Field, Secret};

/// A note we have just created and must be able to spend again.
#[derive(Debug)]
pub struct Minted {
    pub commitment: Field,
    pub value: u64,
    pub asset: Field,
    pub random: Secret,
    /// Set when the note belongs to a session owner rather than this wallet's.
    /// It is the one thing a session delegate is handed, so losing it costs
    /// the note the same way losing `random` does.
    pub seed: Option<Secret>,
}

/// A note the index has placed in the tree.
#[derive(Debug)]
pub struct Held {
    pub commitment: Field,
    /// Position, which is what a spend needs to build its path.
    pub leaf: u32,
    /// What a session reserves on chain, and only this owner can name.
    pub nullifier: Field,
    pub value: u64,
    pub asset: Field,
    pub random: Secret,
}

/// A note we created that the chain has not placed yet.
#[derive(Debug)]
pub struct Unplaced {
    pub commitment: Field,
    /// When it was written down, so one that never lands can be noticed.
    pub created: u64,
    /// Which owner it belongs to, which decides the nullifier it will reveal.
    pub seed: Option<Secret>,
}

/// What one reconciliation pass changed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Reconciled {
    /// Notes the index has now placed.
    pub placed: usize,
    /// Notes the pool has nullified.
    pub spent: usize,
    /// Holds extended to the deadline of an escrow the pool has opened.
    pub escrowed: usize,
    /// Notes still unplaced long after they were written down, which means a
    /// transaction never landed. Counted, never deleted: the row is the only
    /// copy of what makes the note spendable.
    pub stale: usize,
}
