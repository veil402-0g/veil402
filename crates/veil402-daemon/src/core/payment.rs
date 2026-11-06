//! Sending a private payment.
//!
//! The same shape as opening an envelope, without the escrow: reserve a note,
//! prove the transfer, broadcast it through the relay so no address of ours
//! appears, wait for it to land, and record it. The recipient's own scan
//! finds what was sealed to them.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use thiserror::Error;
use tokio::time::sleep;
use veil402_sdk::{Field, evm::PoolClient, session::PaymentAddress};

use crate::core::{
    StoreError,
    envelope::{Broadcaster, ChainError, Landed, ProveError, Prover},
    index::{IndexError, IndexService},
    receipts::{Direction, Receipt, Receipts},
    wallet::{Held, WalletError, WalletService},
};

/// How long a submission may take to land before the attempt is abandoned.
const PATIENCE: Duration = Duration::from_mins(2);

/// Gap between asking whether the payment has landed.
const POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum PayError {
    #[error(transparent)]
    Wallet(#[from] WalletError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    Prove(#[from] ProveError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The note has no path yet: the index has not placed it.
    #[error("the note being paid is not placed")]
    Unplaced,
    /// The payment was replaced or never mined.
    #[error("the payment did not land")]
    Abandoned,
    /// The pool rejected it on chain.
    #[error("the pool rejected the payment: {0}")]
    Rejected(String),
}

impl PayError {
    #[must_use]
    pub fn invalid(&self) -> bool {
        matches!(self, Self::Wallet(WalletError::Unpayable(_) | WalletError::Empty))
    }
}

/// What a sent payment produced.
pub struct Sent {
    pub recipient_commitment: Field,
    /// The recipient note's blinding, the payer's proof of this payment.
    pub blinding: [u8; 32],
    pub change: Option<Field>,
}

/// Sends private payments to another wallet's address.
pub struct PaymentService {
    wallet: Arc<WalletService>,
    index: Arc<IndexService>,
    prover: Arc<dyn Prover>,
    broadcast: Arc<dyn Broadcaster>,
    receipts: Arc<dyn Receipts>,
    pool: PoolClient,
}

impl PaymentService {
    #[must_use]
    pub const fn new(
        wallet: Arc<WalletService>,
        index: Arc<IndexService>,
        prover: Arc<dyn Prover>,
        broadcast: Arc<dyn Broadcaster>,
        receipts: Arc<dyn Receipts>,
        pool: PoolClient,
    ) -> Self {
        Self {
            wallet,
            index,
            prover,
            broadcast,
            receipts,
            pool,
        }
    }

    /// Pays `amount` to `to`, as one shielded transfer.
    ///
    /// # Errors
    ///
    /// Returns [`PayError`] when no note covers the amount, or proving,
    /// broadcasting, or landing fails. The reserved note is let go on failure.
    pub async fn pay(&self, to: PaymentAddress, amount: u64, now: u64) -> Result<Sent, PayError> {
        let note = self.wallet.reserve(self.pool.asset(), amount, now).await?;
        match self.send(&note, &to, amount, now).await {
            Ok(sent) => Ok(sent),
            Err(error) => {
                let _ = self.wallet.release(note.commitment).await;
                Err(error)
            }
        }
    }

    async fn send(
        &self,
        note: &Held,
        to: &PaymentAddress,
        amount: u64,
        now: u64,
    ) -> Result<Sent, PayError> {
        let witness = self.index.witness(&[note.leaf]).await?;
        let path = witness.paths.into_iter().next().ok_or(PayError::Unplaced)?;
        let paid = self.wallet.pay(note, path, to, amount, now).await?;
        let prepared = self.prover.transfer(paid.transaction).await?;
        self.broadcast
            .submit(paid.recipient_commitment, prepared.calldata)
            .await?;
        self.landed(paid.recipient_commitment).await?;
        self.receipts
            .record(&Receipt {
                direction: Direction::Paid,
                amount,
                commitment: paid.recipient_commitment,
                at: now,
            })
            .await?;
        Ok(Sent {
            recipient_commitment: paid.recipient_commitment,
            blinding: paid.recipient_blinding,
            change: paid.change,
        })
    }

    async fn landed(&self, key: Field) -> Result<(), PayError> {
        let since = Instant::now();
        loop {
            match self.broadcast.landed(key).await? {
                Some(Landed::Confirmed { .. }) => return Ok(()),
                Some(Landed::Failed(reason)) => return Err(PayError::Rejected(reason)),
                Some(Landed::Dropped) => return Err(PayError::Abandoned),
                None if since.elapsed() > PATIENCE => return Err(PayError::Abandoned),
                None => sleep(POLL).await,
            }
        }
    }
}
