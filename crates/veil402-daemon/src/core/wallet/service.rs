use std::sync::Arc;

use thiserror::Error;
use borsh::BorshDeserialize;
use veil402_sdk::{
    Field, MerklePath, Note, Opening, Owner, Secret, SessionKeys, Send, Spend, Transaction,
    session::{Sealed, Viewing, seal_note},
};

use super::{ChainFacts, Held, Minted, NoteStore, Reconciled, Recovered, Unopened};
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
    /// Nothing spendable. Only a deposit fixes this: with no size to meet,
    /// the only way to have no note is to have none.
    #[error("no spendable note")]
    Empty,
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

/// A note to run a session on, by position.
///
/// A seeded note that covers `wanted` is the one to want: the last
/// settlement minted it for exactly this, and handing it over costs nothing.
/// Smallest sufficient, so a lock freezes no more of the balance than the
/// session needs. Anything else has to be reseeded first, which costs a
/// proof and a transaction, and is only worth doing against the largest
/// note there is.
///
/// # Errors
///
/// Returns [`WalletError::Empty`] when nothing is spendable.
pub fn choose(held: &[Held], wanted: u64) -> Result<usize, WalletError> {
    held.iter()
        .enumerate()
        .filter(|(_, note)| note.seed.is_some() && note.value >= wanted)
        .min_by_key(|(_, note)| note.value)
        .or_else(|| held.iter().enumerate().max_by_key(|(_, note)| note.value))
        .map(|(position, _)| position)
        .ok_or(WalletError::Empty)
}

/// A wallet note given an identity of its own.
pub struct Reseeded {
    /// Ready to prove and broadcast.
    pub transaction: Transaction,
    /// Commitment of the session note, which is also the key its submission
    /// is tracked under.
    pub session: Field,
}

/// Mints the whole of `note` again under a key derived from one seed.
///
/// Whole rather than a piece of it: the session's ceiling is whatever the
/// note holds, so there is no size to cut to, and a remainder would only be
/// a second output to prove and write down.
fn reseeded(note: &Held, view: [u8; 32]) -> Result<(Minted, Send), veil402_sdk::Error> {
    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    output(
        note.asset,
        note.value,
        keys.owner.public()?,
        &keys.random,
        Some(&seed),
        view,
    )
}

/// How many unopened leaves one pass tries. Bounded so a fresh wallet
/// catching up on a long history does not hold the pass open.
const SCAN: u32 = 512;

/// Splits the encapsulated key from the ciphertext the pool carried.
fn split(note: &[u8]) -> Option<Sealed> {
    let (head, ciphertext) = note.split_at_checked(32)?;
    Some(Sealed {
        encapsulated: head.try_into().ok()?,
        ciphertext: ciphertext.to_vec(),
    })
}

/// Seals a note's opening to the wallet that will own it, so the note can
/// be found again from the viewing key alone.
///
/// A session note's blinding comes from its seed, so the seed is what
/// travels and the blinding is derived back from it.
fn sealed(
    view: [u8; 32],
    value: u64,
    asset: Field,
    random: &Secret,
    seed: Option<&Secret>,
) -> Result<Vec<u8>, veil402_sdk::Error> {
    let opening = Opening {
        value,
        asset: asset.to_bytes(),
        secret: seed.unwrap_or(random).to_bytes(),
        session: seed.is_some(),
    };
    let sealed = seal_note(
        view,
        &borsh::to_vec(&opening)
            .map_err(|_| veil402_sdk::Error::Transaction("a note opening will not encode"))?,
    )?;
    let mut bytes = Vec::with_capacity(32 + sealed.ciphertext.len());
    bytes.extend_from_slice(&sealed.encapsulated);
    bytes.extend_from_slice(&sealed.ciphertext);
    Ok(bytes)
}

/// One output, as the row to keep and the send to prove. Both carry the same
/// blinding, or the note written down is not the note created.
fn output(
    asset: Field,
    value: u64,
    owner: Field,
    random: &Secret,
    seed: Option<&Secret>,
    view: [u8; 32],
) -> Result<(Minted, Send), veil402_sdk::Error> {
    let kept = mint(
        asset,
        value,
        owner,
        Secret::from_bytes(random.to_bytes())?,
        seed.map(|seed| Secret::from_bytes(seed.to_bytes()))
            .transpose()?,
    )?;
    let created = Send::new(
        owner,
        value,
        Secret::from_bytes(random.to_bytes())?,
        sealed(view, value, asset, random, seed)?,
    )?;
    Ok((kept, created))
}

/// The note a settlement minted, derived the way the enclave derived it.
/// The two must agree exactly: a mismatch loses the change outright.
fn outputs(
    note: &Held,
    spent: u64,
    change_seed: &Secret,
) -> Result<Vec<Minted>, veil402_sdk::Error> {
    let change = note.value.saturating_sub(spent);
    if change == 0 {
        return Ok(Vec::new());
    }
    let keys = SessionKeys::from_seed(change_seed)?;
    let seed = Secret::from_bytes(change_seed.to_bytes())?;
    Ok(vec![mint(
        note.asset,
        change,
        keys.owner.public()?,
        keys.random,
        Some(seed),
    )?])
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
/// Which notes we hold, and which of them a session may use.
pub struct WalletService {
    notes: Arc<dyn NoteStore>,
    chain: Arc<dyn ChainFacts>,
    owner: Owner,
}

impl WalletService {
    /// Where a sender seals a note's opening so this wallet can find it.
    #[must_use]
    pub fn view(&self) -> [u8; 32] {
        Viewing::new(&self.owner.view).public()
    }

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
    pub async fn reserve(&self, asset: Field, wanted: u64, now: u64) -> Result<Held, WalletError> {
        let mut free = self.notes.spendable(asset, now).await?;
        loop {
            let note = free.swap_remove(choose(&free, wanted)?);
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

    /// Gives a wallet note an identity of its own.
    ///
    /// A note belongs to this wallet's key, and that key spends every note
    /// the wallet holds — so handing one over would hand over all of them.
    /// The reseeded note is owned by a key derived from a single seed, and
    /// nothing else was ever minted to that key, so the seed hands over
    /// exactly one note.
    ///
    /// Written down before the transaction is built, for the same reason
    /// every mint is: nothing recovers it afterwards.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the store cannot be written or the note
    /// cannot be derived.
    pub async fn reseed(
        &self,
        note: &Held,
        merkle: MerklePath,
        now: u64,
    ) -> Result<Reseeded, WalletError> {
        let (minted, send) = reseeded(note, self.view()).map_err(|_| WalletError::Owner)?;
        let session = minted.commitment;
        self.notes.remember(&[minted], now).await?;
        let input = Spend {
            note: Note {
                value: note.value,
                asset: note.asset,
                random: Secret::from_bytes(note.random.to_bytes())
                    .map_err(|_| WalletError::Owner)?,
            },
            owner: self.owner_for(note)?,
            merkle,
        };
        Ok(Reseeded {
            transaction: Transaction::new(vec![input], vec![send])
                .map_err(|_| WalletError::Owner)?,
            session,
        })
    }

    /// A note by the commitment that created it.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Store`] when the store cannot be read.
    pub async fn by_commitment(&self, commitment: Field) -> Result<Option<Held>, WalletError> {
        Ok(self.notes.by_commitment(commitment).await?)
    }

    /// The deadline the pool holds against a nullifier, if it holds one.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Store`] when the chain's record cannot be read.
    pub async fn escrow(&self, nullifier: Field) -> Result<Option<u64>, WalletError> {
        Ok(self.chain.escrow(nullifier).await?)
    }

    /// The key that signs for a note: its own session owner, or this wallet's.
    fn owner_for(&self, note: &Held) -> Result<Owner, WalletError> {
        match &note.seed {
            Some(seed) => SessionKeys::from_seed(seed)
                .map(|keys| keys.owner)
                .map_err(|_| WalletError::Owner),
            None => Ok(Owner {
                spend: Secret::from_bytes(self.owner.spend.to_bytes())
                    .map_err(|_| WalletError::Owner)?,
                view: Secret::from_bytes(self.owner.view.to_bytes())
                    .map_err(|_| WalletError::Owner)?,
            }),
        }
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
        now: u64,
    ) -> Result<Recovered, WalletError> {
        let Some(note) = self.notes.by_nullifier(nullifier).await? else {
            return Ok(Recovered::Nothing);
        };
        let Some(spent) = self.chain.settlement(nullifier).await? else {
            return Ok(Recovered::Pending);
        };
        // A settlement to an address always pays something, so a zero means
        // the seller took a note instead and the amount stayed private.
        // Deriving against it would mint commitments the chain never saw.
        if spent == 0 {
            return Ok(Recovered::Nothing);
        }
        let minted = outputs(&note, spent, change_seed)
            .map_err(|_| WalletError::Owner)?;
        let change = minted.first().map(|note| note.commitment);
        self.notes.remember(&minted, now).await?;
        Ok(change.map_or(Recovered::Nothing, Recovered::Change))
    }

    /// Finds notes on chain that this wallet owns but has no record of.
    ///
    /// Every opening is sealed to the viewing key and emitted beside its
    /// commitment, so a wallet that lost its database — or never had one —
    /// recovers by opening what it can and ignoring what it cannot. Each
    /// leaf is tried once, ever, so the scan does not grow with the pool.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Store`] when the store cannot be read.
    pub async fn discover(&self, now: u64) -> Result<u32, WalletError> {
        let viewing = Viewing::new(&self.owner.view);
        let from = self.chain.scan_mark().await?;
        let unopened = self.chain.unopened(from, SCAN).await?;
        let mut found = 0;
        let mut next = from;
        for leaf in &unopened {
            next = next.max(leaf.leaf_index.saturating_add(1));
            if let Some(minted) = self.opened(&viewing, leaf)? {
                self.notes.remember(&[minted], now).await?;
                found += 1;
            }
        }
        if next > from {
            self.chain.scanned(next).await?;
        }
        Ok(found)
    }

    /// One commitment, if it opens to a note this wallet owns.
    ///
    /// A ciphertext that will not open was sealed to somebody else, which is
    /// the ordinary case. One that opens onto another commitment was sealed
    /// to us dishonestly, and is refused rather than written down.
    fn opened(&self, viewing: &Viewing, leaf: &Unopened) -> Result<Option<Minted>, WalletError> {
        let Some(sealed) = split(&leaf.note) else {
            return Ok(None);
        };
        let Ok(plain) = viewing.open(&sealed) else {
            return Ok(None);
        };
        let Ok(opening) = Opening::try_from_slice(&plain) else {
            return Ok(None);
        };
        let asset = Field::from_bytes(opening.asset).map_err(|_| WalletError::Owner)?;
        let secret = Secret::from_bytes(opening.secret).map_err(|_| WalletError::Owner)?;
        let (owner, random, seed) = if opening.session {
            let keys = SessionKeys::from_seed(&secret).map_err(|_| WalletError::Owner)?;
            (
                keys.owner.public().map_err(|_| WalletError::Owner)?,
                keys.random,
                Some(secret),
            )
        } else {
            (
                self.owner.public().map_err(|_| WalletError::Owner)?,
                secret,
                None,
            )
        };
        let minted = mint(asset, opening.value, owner, random, seed).map_err(|_| WalletError::Owner)?;
        Ok((minted.commitment == leaf.commitment).then_some(minted))
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
