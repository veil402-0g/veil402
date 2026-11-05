use std::sync::Arc;

use thiserror::Error;
use veil402_sdk::{Field, Owner, Secret, SessionKeys};

use super::{ChainFacts, Held, Minted, NoteStore, Reconciled};
use crate::core::StoreError;

/// A note still unplaced after this long means its transaction never landed.
const STALE: u64 = 3_600;

/// How long a reservation holds a note on its own.
///
/// Only long enough for the lock it was taken for to land and be indexed. If
/// it lands, the escrow's own deadline takes over; if it never lands, the hold
/// simply lapses, which is why nothing has to sweep it up.
const INTENT: u64 = 120;

/// Mirrors `Pool.SETTLE_GRACE`: a reservation outlives its deadline, so a
/// settlement landing late cannot collide with the note being taken back.
const GRACE: u64 = 60;

#[derive(Debug, Error)]
pub enum WalletError {
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The balance covers the session but no single note does. A split fixes
    /// this; a deposit does not.
    #[error("no note covers {ceiling}: {balance} held across {notes} notes")]
    Fragmented {
        ceiling: u64,
        balance: u64,
        notes: usize,
    },
    #[error("{balance} held, {ceiling} needed")]
    Insufficient { ceiling: u64, balance: u64 },
    #[error("the owner key cannot derive a nullifier")]
    Owner,
}

impl WalletError {
    /// Whether trying again could plausibly succeed, on the same terms the
    /// index sorts its own failures.
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        matches!(self, Self::Store(StoreError::Unavailable))
    }
}

/// The smallest note that covers `ceiling` on its own, by position.
///
/// A settlement the buyer cannot prove leaves the seller unpaid for work it
/// has already delivered, so one note has to cover the session without needing
/// a second input. Smallest sufficient keeps the rest of the balance free: a
/// lock freezes its note for the whole deadline, not just for a block.
///
/// # Errors
///
/// Returns [`WalletError::Fragmented`] when the balance is there but no single
/// note holds it, and [`WalletError::Insufficient`] when it is not.
pub fn select(held: &[Held], ceiling: u64) -> Result<usize, WalletError> {
    held.iter()
        .enumerate()
        .filter(|(_, note)| note.value >= ceiling)
        .min_by_key(|(_, note)| note.value)
        .map(|(position, _)| position)
        .ok_or_else(|| shortfall(held, ceiling))
}

/// Says whether a split or a deposit is what is missing, since only one of
/// them is something the operator has to fund.
fn shortfall(held: &[Held], ceiling: u64) -> WalletError {
    let balance = held.iter().map(|note| note.value).sum();
    if balance >= ceiling {
        WalletError::Fragmented {
            ceiling,
            balance,
            notes: held.len(),
        }
    } else {
        WalletError::Insufficient { ceiling, balance }
    }
}

/// Divides change so a settlement leaves a session-sized note behind.
///
/// A settlement emits two commitments either way and pads the spare one with a
/// dummy, so the second note is free: it is an output slot already paid for.
/// Splitting is what lets the next session run while this note is locked, and
/// it keeps the public settlement amount from matching any public deposit.
#[must_use]
pub fn split(change: u64, target: u64) -> Vec<u64> {
    if change == 0 {
        return Vec::new();
    }
    if target > 0 && change >= target.saturating_mul(2) {
        return vec![target, change - target];
    }
    vec![change]
}

/// Which notes we hold, and which of them a session may use.
pub struct WalletService {
    notes: Arc<dyn NoteStore>,
    chain: Arc<dyn ChainFacts>,
    owner: Owner,
}

impl WalletService {
    #[must_use]
    pub const fn new(notes: Arc<dyn NoteStore>, chain: Arc<dyn ChainFacts>, owner: Owner) -> Self {
        Self {
            notes,
            chain,
            owner,
        }
    }

    /// Takes a note out of circulation for a session and hands it over.
    ///
    /// The hold is taken here, before anything reaches the chain: two sessions
    /// that both read the same free note would race, and whichever lost could
    /// not settle. It is deliberately short, and not the caller's to choose —
    /// once the lock lands, the escrow's deadline replaces it.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when no note covers the ceiling on its own, or
    /// the store cannot be reached.
    pub async fn reserve(&self, asset: Field, ceiling: u64, now: u64) -> Result<Held, WalletError> {
        let mut free = self.notes.spendable(asset, now).await?;
        loop {
            let note = free.swap_remove(select(&free, ceiling)?);
            if self
                .notes
                .hold(note.commitment, now, now.saturating_add(INTENT))
                .await?
            {
                return Ok(note);
            }
        }
    }

    /// Lets a note go, for a caller that knows the lock it was taken for will
    /// never land. Skipping this only costs the wait for the hold to lapse.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Store`] when the store cannot be written.
    pub async fn release(&self, commitment: Field) -> Result<(), WalletError> {
        Ok(self.notes.free(commitment).await?)
    }

    /// Writes notes down before the transaction creating them is sent.
    ///
    /// Written afterwards instead, a crash in between loses them outright:
    /// their ciphertexts are not decryptable, so nothing recovers them.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Store`] when the store cannot be written.
    pub async fn remember(&self, minted: &[Minted], now: u64) -> Result<(), WalletError> {
        Ok(self.notes.remember(minted, now).await?)
    }

    /// Catches the notes up with the chain: places the ones the index has
    /// seen, retires the ones the pool has nullified, and hands a held note
    /// over to the escrow the pool opened against it.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the chain or the store cannot be read.
    pub async fn reconcile(&self, now: u64) -> Result<Reconciled, WalletError> {
        let mut pass = Reconciled::default();
        for note in self.notes.unplaced().await? {
            match self.chain.leaf_of(note.commitment).await? {
                Some(leaf) => {
                    let nullifier = self.nullifier(note.seed.as_ref(), leaf)?;
                    self.notes.place(note.commitment, leaf, nullifier).await?;
                    pass.placed += 1;
                }
                None if now.saturating_sub(note.created) > STALE => pass.stale += 1,
                None => {}
            }
        }

        let mut spent = Vec::new();
        for note in self.notes.outstanding().await? {
            if self.chain.is_spent(note.nullifier).await? {
                spent.push(note.nullifier);
            } else if let Some(deadline) = self.chain.escrow(note.nullifier).await? {
                // The pool's own record of the reservation, which outlasts the
                // short hold `reserve` took on faith.
                let until = deadline.saturating_add(GRACE);
                if self.notes.extend(note.commitment, until).await? {
                    pass.escrowed += 1;
                }
            }
        }
        if !spent.is_empty() {
            pass.spent = spent.len();
            self.notes.spend(&spent).await?;
        }
        Ok(pass)
    }

    /// A session note belongs to its own owner, so the nullifier it reveals is
    /// not the one this wallet's key derives. Using the wrong one writes a
    /// nullifier the pool will never record, and the note is then offered
    /// forever.
    fn nullifier(&self, seed: Option<&Secret>, leaf: u32) -> Result<Field, WalletError> {
        match seed {
            Some(seed) => SessionKeys::from_seed(seed).and_then(|keys| keys.owner.nullifier(leaf)),
            None => self.owner.nullifier(leaf),
        }
        .map_err(|_| WalletError::Owner)
    }
}
