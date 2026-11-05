use std::sync::Arc;

use thiserror::Error;
use veil402_sdk::{Field, Note, Owner, Secret, SessionKeys, change_random, split};

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

/// The notes a settlement minted, derived the way the enclave derived them.
/// The two must agree exactly: a mismatch loses the change outright.
fn outputs(
    note: &Held,
    spent: u64,
    change_seed: &Secret,
    owner: &Owner,
    target: u64,
) -> Result<Vec<Minted>, veil402_sdk::Error> {
    let mut amounts = split(note.value.saturating_sub(spent), target).into_iter();
    let mut minted = Vec::new();
    if let Some(value) = amounts.next() {
        let keys = SessionKeys::from_seed(change_seed)?;
        let seed = Secret::from_bytes(change_seed.to_bytes())?;
        minted.push(mint(note.asset, value, keys.owner.public()?, keys.random, Some(seed))?);
    }
    if let Some(value) = amounts.next() {
        let random = change_random(change_seed)?;
        minted.push(mint(note.asset, value, owner.public()?, random, None)?);
    }
    Ok(minted)
}

fn mint(
    asset: Field,
    value: u64,
    owner: Field,
    random: Secret,
    seed: Option<Secret>,
) -> Result<Minted, veil402_sdk::Error> {
    let note = Note { value, asset, random };
    Ok(Minted {
        commitment: note.commitment_for(owner)?,
        value,
        asset,
        random: note.random,
        seed,
    })
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

    /// Rebuilds the notes a settlement left behind and writes them down.
    ///
    /// The seller settles, not the buyer, so this runs against a chain the
    /// daemon may have been absent for. All it needs is the seed it kept and
    /// the amount the pool recorded: both outputs derive, and nothing has to
    /// be decrypted.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the chain or the store cannot be read, or
    /// a note cannot be derived.
    pub async fn recover(
        &self,
        nullifier: Field,
        change_seed: &Secret,
        target: u64,
        now: u64,
    ) -> Result<usize, WalletError> {
        let (Some(spent), Some(note)) = (
            self.chain.settlement(nullifier).await?,
            self.notes.by_nullifier(nullifier).await?,
        ) else {
            return Ok(0);
        };
        // A settlement to an address always pays something, so a zero means
        // the seller took a note instead and the amount stayed private.
        // Deriving against it would mint commitments the chain never saw.
        if spent == 0 {
            return Ok(0);
        }
        let minted = outputs(&note, spent, change_seed, &self.owner, target)
            .map_err(|_| WalletError::Owner)?;
        let placed = minted.len();
        self.notes.remember(&minted, now).await?;
        Ok(placed)
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
