use async_trait::async_trait;
use veil402_sdk::Field;

use super::{Held, Minted, Unplaced};
use crate::core::StoreError;

/// Holds the notes we own.
///
/// Note ciphertexts are not decryptable yet, so nothing here can be recovered
/// from the chain: this store is the only copy of what makes a note spendable.
#[async_trait]
pub trait NoteStore: Send + Sync {
    /// Records notes before the transaction that creates them is sent.
    async fn remember(&self, minted: &[Minted], now: u64) -> Result<(), StoreError>;

    /// Notes the chain has not placed in the tree yet.
    async fn unplaced(&self) -> Result<Vec<Unplaced>, StoreError>;

    /// Gives a note its position and the nullifier only its owner can derive.
    async fn place(&self, commitment: Field, leaf: u32, nullifier: Field)
    -> Result<(), StoreError>;

    /// Every placed, unspent note, whatever its asset or hold.
    async fn outstanding(&self) -> Result<Vec<Held>, StoreError>;

    /// Retires notes the pool has nullified.
    async fn spend(&self, nullifiers: &[Field]) -> Result<(), StoreError>;

    /// Notes free to back a session: placed, unspent, and not held at `now`.
    async fn spendable(&self, asset: Field, now: u64) -> Result<Vec<Held>, StoreError>;

    /// Holds a note back until `until`, and says whether this call is the one
    /// that took it. Two sessions reading the same free note must not both
    /// get it, so the claim is the decision, not the read before it.
    async fn hold(&self, commitment: Field, now: u64, until: u64) -> Result<bool, StoreError>;

    /// Extends a hold to the deadline the pool itself recorded, if it is not
    /// already there, and says whether it moved. Idempotent, so a pass that
    /// changes nothing reports nothing.
    async fn extend(&self, commitment: Field, until: u64) -> Result<bool, StoreError>;

    /// Lets a note go, for a caller that knows its lock will never land.
    async fn free(&self, commitment: Field) -> Result<(), StoreError>;
}

/// What the wallet needs the chain to tell it.
///
/// Narrow on purpose: the index owns these facts, but the wallet has no
/// business appending to it.
#[async_trait]
pub trait ChainFacts: Send + Sync {
    /// Where a commitment landed, once the pool has emitted it.
    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError>;

    /// Whether the pool has recorded this nullifier.
    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError>;

    /// The deadline of the escrow the pool holds against this nullifier, if
    /// it holds one. This is the chain's own record of our reservation.
    async fn escrow(&self, nullifier: Field) -> Result<Option<u64>, StoreError>;
}
