use veil402_sdk::Field;

/// A commitment the pool emitted, at the leaf index it was inserted at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commitment {
    /// Position in the tree, which is what a spend needs to build its path.
    pub leaf_index: u32,
    /// The commitment itself.
    pub commitment: Field,
    /// Ciphertext the pool carried alongside it.
    pub note: Vec<u8>,
    /// Block that carried it. The chain answers only a fixed window, so this
    /// cannot be recovered once that window has passed.
    pub block: u64,
}

/// A nullifier the pool recorded, at the block that carried it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Nullified {
    /// The nullifier itself.
    pub nullifier: Field,
    /// Block that carried it.
    pub block: u64,
}

/// An escrow the pool opened, at the block that carried it.
///
/// Only a note's owner can name its nullifier, so an escrow here is one we
/// opened: it is the chain's record of our own reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Locked {
    /// Nullifier the escrow reserves.
    pub nullifier: Field,
    /// When the reservation lapses.
    pub deadline: u64,
    /// Block that carried it.
    pub block: u64,
}

/// What one sync pass observed, in the order the chain applied it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Batch {
    /// New commitments, ordered by leaf index.
    pub commitments: Vec<Commitment>,
    /// Nullifiers the pool recorded.
    pub nullifiers: Vec<Nullified>,
    /// Escrows the pool opened.
    pub locks: Vec<Locked>,
    /// Last block this pass covers.
    pub through: u64,
}

impl Batch {
    /// How many events this pass carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commitments.len() + self.nullifiers.len() + self.locks.len()
    }

    /// Whether the pass found nothing. A pass can still advance the cursor
    /// through blocks that held no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The index's standing against the pool.
///
/// Being behind and being wrong are different things, and only one of them
/// matters: the pool keeps every root it has ever reached, so a proof built
/// against an older one is still accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Health {
    /// Commitments indexed.
    pub leaves: u32,
    /// Block the index has read to.
    pub block: u64,
    /// Root the local tree derives.
    pub root: Field,
}
