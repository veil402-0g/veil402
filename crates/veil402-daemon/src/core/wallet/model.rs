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
    /// Set when the note belongs to a session owner. It is what a delegate is
    /// handed, so a session note cannot be reserved without it.
    pub seed: Option<Secret>,
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

/// What a settlement left behind.
///
/// Three answers rather than a count, because a caller waiting on its change
/// has to tell "not published yet" from "there is none".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recovered {
    /// The chain has not published this settlement yet.
    Pending,
    /// Nothing came back: the note was spent to its last unit, or the seller
    /// took a note and the amount stayed private.
    Nothing,
    /// What came back, written down and waiting for the index to place it.
    Change(Field),
}

/// What the wallet is worth, and across how many notes.
///
/// The count is not decoration: the balance is spendable only in whole
/// notes, so one note of a thousand and a thousand notes of one spend very
/// differently.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Holdings {
    pub balance: u64,
    pub notes: u32,
}

/// A shield, which is the one public thing a buyer does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deposit {
    pub commitment: Field,
    pub amount: u64,
    pub at: u64,
}

/// A note the scan found on chain that this wallet owns but had no record
/// of — an incoming payment, as far as a healthy wallet is concerned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Received {
    pub value: u64,
    pub commitment: Field,
}

/// A commitment the wallet has not yet tried to open.
#[derive(Clone, Debug)]
pub struct Unopened {
    pub leaf_index: u32,
    pub commitment: Field,
    /// The ciphertext the pool carried beside it.
    pub note: Vec<u8>,
}
