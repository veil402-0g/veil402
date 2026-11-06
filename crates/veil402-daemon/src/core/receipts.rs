//! Payments the chain does not name.
//!
//! A shielded transfer leaves only commitments and nullifiers on chain, so a
//! console cannot read a payment back from it. The daemon that made or found
//! one writes it here, which is the only record that it was a payment at all.

use async_trait::async_trait;
use veil402_sdk::Field;

use crate::core::StoreError;

/// Which way value moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Paid,
    Received,
}

/// A private payment this wallet sent or found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub direction: Direction,
    pub amount: u64,
    /// The output commitment that carried it: the recipient's for a payment
    /// sent, the note's own for one received.
    pub commitment: Field,
    pub at: u64,
}

/// Remembers payments so the operator feed can show them.
#[async_trait]
pub trait Receipts: Send + Sync {
    async fn record(&self, receipt: &Receipt) -> Result<(), StoreError>;

    /// The most recent payments, newest first.
    async fn receipts(&self, limit: u32) -> Result<Vec<Receipt>, StoreError>;
}
