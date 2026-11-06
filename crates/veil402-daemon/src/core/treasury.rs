//! Shielding funds, and saying how far along it is.
//!
//! A deposit is the one public thing a buyer does, and the slowest: an
//! allowance the first time, a transfer, and then waiting for the pool to
//! place the note. Each of those is a step somebody is watching, so the
//! service keeps where it has got to rather than returning only at the end.

use std::sync::Mutex;

use alloy::primitives::Address;
use async_trait::async_trait;
use serde::Serialize;
use veil402_sdk::{Field, Note, Owner, Secret, evm::PoolClient};

use crate::core::{
    envelope::ChainError,
    wallet::{Deposit, Minted, WalletError, WalletService},
};

/// How far a deposit has got.
///
/// Ordered as it happens, so a console can draw them as steps and know
/// which are behind it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Letting the pool move this many tokens. Once only, per key.
    Approving,
    /// The transfer itself.
    Shielding,
    /// Written down, waiting for the pool to place it in the tree.
    Placing,
    Done,
    Failed,
}

/// A deposit in flight, or the last one there was.
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub stage: Stage,
    /// Pool units being shielded.
    pub amount: u64,
    pub started: u64,
    /// Set once the note exists, which is what a spend will name.
    pub commitment: Option<String>,
    /// Why it stopped, when it stopped badly.
    pub error: Option<String>,
}

impl Progress {
    fn new(amount: u64, now: u64) -> Self {
        Self {
            stage: Stage::Approving,
            amount,
            started: now,
            commitment: None,
            error: None,
        }
    }
}

/// Sends the calls a deposit makes.
///
/// Wider than the pool: shielding means letting the pool move tokens
/// first, which is a call to the token.
#[async_trait]
pub trait Funder: Send + Sync {
    /// The address paying, and whose tokens are shielded.
    fn account(&self) -> Address;

    /// Lets `spender` move at least `amount`, and says whether it had to
    /// ask. Encoding that permission is the token's business, not this
    /// service's.
    async fn allow(&self, spender: Address, amount: u64) -> Result<bool, ChainError>;

    /// Sends one call and waits for the chain to take it.
    async fn send(&self, to: Address, calldata: Vec<u8>) -> Result<(), ChainError>;
}

#[derive(Debug, thiserror::Error)]
pub enum TreasuryError {
    #[error("a deposit is already running")]
    Busy,
    #[error("a deposit of nothing is not one")]
    Nothing,
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    Wallet(#[from] WalletError),
    #[error("the note could not be built: {0}")]
    Note(String),
}

/// Shields funds into the pool under this daemon's own key.
pub struct TreasuryService {
    wallet: std::sync::Arc<WalletService>,
    funder: std::sync::Arc<dyn Funder>,
    pool: PoolClient,
    owner: Owner,
    /// The deposit in flight, or the last one. One at a time: they share a
    /// nonce, and two racing for it is how one of them vanishes.
    progress: Mutex<Option<Progress>>,
}

impl TreasuryService {
    #[must_use]
    pub fn new(
        wallet: std::sync::Arc<WalletService>,
        funder: std::sync::Arc<dyn Funder>,
        pool: PoolClient,
        owner: Owner,
    ) -> Self {
        Self {
            wallet,
            funder,
            pool,
            owner,
            progress: Mutex::new(None),
        }
    }

    /// Where the current or last deposit got to.
    #[must_use]
    pub fn progress(&self) -> Option<Progress> {
        self.progress.lock().ok().and_then(|held| held.clone())
    }

    fn set(&self, stage: Stage, commitment: Option<String>, error: Option<String>) {
        if let Ok(mut held) = self.progress.lock()
            && let Some(at) = held.as_mut()
        {
            at.stage = stage;
            if commitment.is_some() {
                at.commitment = commitment;
            }
            at.error = error;
        }
    }

    /// Claims the one deposit slot, so a caller learns it cannot start
    /// before anything is sent.
    ///
    /// # Errors
    ///
    /// Returns [`TreasuryError`] when one is already running or the amount
    /// is nothing.
    pub fn begin(&self, amount: u64, now: u64) -> Result<(), TreasuryError> {
        if amount == 0 {
            return Err(TreasuryError::Nothing);
        }
        let mut held = self.progress.lock().map_err(|_| TreasuryError::Busy)?;
        if held.as_ref().is_some_and(|at| {
            matches!(at.stage, Stage::Approving | Stage::Shielding | Stage::Placing)
        }) {
            return Err(TreasuryError::Busy);
        }
        *held = Some(Progress::new(amount, now));
        Ok(())
    }

    /// Shields `amount`, reporting each step as it goes. Call [`Self::begin`]
    /// first: it is what decides whether this may run at all.
    ///
    /// # Errors
    ///
    /// Returns [`TreasuryError`] when the chain refuses a call.
    pub async fn fund(&self, amount: u64, now: u64) -> Result<Field, TreasuryError> {
        match self.shield(amount, now).await {
            Ok(commitment) => {
                self.set(Stage::Done, Some(veil402_sdk::wire::encode(&commitment.to_bytes())), None);
                Ok(commitment)
            }
            Err(error) => {
                self.set(Stage::Failed, None, Some(error.to_string()));
                Err(error)
            }
        }
    }

    async fn shield(&self, amount: u64, now: u64) -> Result<Field, TreasuryError> {
        // Approved once and left standing: the pool is the only spender this
        // key ever names, and an allowance per deposit is a transaction per
        // deposit for a permission that does not change.
        self.funder.allow(self.pool.address(), amount).await?;

        self.set(Stage::Shielding, None, None);
        let refused = |error: &dyn std::fmt::Display| TreasuryError::Note(error.to_string());
        let note = Note {
            value: amount,
            asset: self.pool.asset(),
            random: Secret::random().map_err(|error| refused(&error))?,
        };
        let commitment = note.commitment(&self.owner).map_err(|error| refused(&error))?;
        // Sealed to our own viewing key, so a lost database can find this
        // note again by scanning. A deposit that carried nothing would be
        // spendable only for as long as the row describing it survived.
        let opening = self
            .wallet
            .seal(amount, self.pool.asset(), &note.random)
            .map_err(|error| refused(&error))?;
        self.funder
            .send(
                self.pool.address(),
                self.pool
                    .shield(
                        note.public_key(&self.owner).map_err(|error| refused(&error))?,
                        amount,
                        &opening,
                    )
                    .map_err(|error| refused(&error))?,
            )
            .await?;

        self.set(Stage::Placing, None, None);
        let random = Secret::from_bytes(note.random.to_bytes()).map_err(|error| refused(&error))?;
        self.wallet
            .remember(
                &[Minted {
                    commitment,
                    value: amount,
                    asset: self.pool.asset(),
                    random,
                    seed: None,
                }],
                now,
            )
            .await?;
        self.wallet
            .deposited(&Deposit {
                commitment,
                amount,
                at: now,
            })
            .await?;
        Ok(commitment)
    }
}
