use std::{sync::Arc, time::Duration};

use alloy::primitives::Address;
use thiserror::Error;
use futures_util::{StreamExt, stream::BoxStream};
use tokio::{
    sync::{Mutex, RwLock},
    time::sleep,
};
use veil402_sdk::{
    Field, Secret,
    evm::{PoolClient, Session},
    session::{Handover, Recipient},
    wire::Offer,
};

use super::{
    Agreement, Answered, Broadcaster, ChainError, Enclave, EnclaveError, Envelope, EnvelopeStore, Landed,
    Piece, ProveError, Prover,
};
use crate::core::{
    StoreError,
    index::{IndexError, IndexService},
    wallet::{Held, WalletError, WalletService},
};

/// How long before a deadline an envelope must stop serving and settle.
///
/// Covers proving, inclusion and reorg margin. The pool allows `SETTLE_GRACE`
/// beyond the deadline, which is the tail rather than the budget.
const BUFFER: u64 = 90;

/// How long a lock may take to land before the attempt is abandoned.
const PATIENCE: u64 = 120;

/// Gap between asking whether something has landed. Waiting is the normal
/// case, and asking as fast as the loop will go only loads the endpoint that
/// has to answer.
const POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum EnvelopeError {
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    Prove(#[from] ProveError),
    #[error(transparent)]
    Wallet(#[from] WalletError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Enclave(#[from] EnclaveError),
    /// The window asked for is longer than the pool will hold a reservation.
    #[error("a lock cannot run for {0} seconds")]
    Window(u64),
    /// The daemon never recorded this envelope, so its change is underivable.
    #[error("no envelope is held for this nullifier")]
    Unknown,
    /// The lock never landed. The note's hold lapses on its own.
    #[error("the lock did not land")]
    Abandoned,
}

impl EnvelopeError {
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        match self {
            Self::Chain(error) => error.recoverable(),
            Self::Prove(error) => error.recoverable(),
            Self::Wallet(error) => error.recoverable(),
            Self::Index(error) => error.recoverable(),
            Self::Store(error) => matches!(error, StoreError::Unavailable),
            Self::Enclave(error) => error.recoverable(),
            _ => false,
        }
    }
}

/// Opens envelopes, meters against them, and settles them.
pub struct EnvelopeService {
    wallet: Arc<WalletService>,
    index: Arc<IndexService>,
    enclave: Arc<dyn Enclave>,
    broadcast: Arc<dyn Broadcaster>,
    prover: Arc<dyn Prover>,
    envelopes: Arc<dyn EnvelopeStore>,
    terms: Terms,
    /// Held while an envelope is being replaced, so it happens once.
    rolling: Mutex<()>,
    /// What the seller sells, as of the first call that needed to know.
    catalogue: RwLock<Option<Vec<Offer>>>,
}

/// What shapes an envelope, beyond the ports it runs over.
#[derive(Clone, Copy, Debug)]
pub struct Terms {
    pub pool: PoolClient,
    /// Who this daemon buys from. The catalogue picks it; re-shopping is a
    /// different value here, not a different code path.
    pub seller: Address,
    /// What a session note wants to be worth.
    ///
    /// Two jobs, one number: the size worth reserving for a session, and the
    /// size a settlement tries to leave behind for the next one. They have
    /// to agree, so there is no sense in letting them disagree.
    pub session: u64,
    /// How long one envelope holds its reservation for.
    pub window: u64,
}

impl EnvelopeService {
    #[must_use]
    pub fn new(
        wallet: Arc<WalletService>,
        index: Arc<IndexService>,
        enclave: Arc<dyn Enclave>,
        broadcast: Arc<dyn Broadcaster>,
        prover: Arc<dyn Prover>,
        envelopes: Arc<dyn EnvelopeStore>,
        terms: Terms,
    ) -> Self {
        Self {
            wallet,
            index,
            enclave,
            broadcast,
            prover,
            envelopes,
            terms,
            rolling: Mutex::new(()),
            catalogue: RwLock::new(None),
        }
    }

    /// What this seller sells.
    ///
    /// Read once and kept: it is the same answer for every agent, and a
    /// model that goes away refuses at the upstream rather than here.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the seller cannot be reached.
    pub async fn offers(&self) -> Result<Vec<Offer>, EnvelopeError> {
        if let Some(offers) = self.catalogue.read().await.as_ref() {
            return Ok(offers.clone());
        }
        let offers = self.enclave.catalogue().await?;
        *self.catalogue.write().await = Some(offers.clone());
        Ok(offers)
    }

    /// The offer a model names, if this seller sells it.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the seller cannot be reached.
    pub async fn offer(&self, model: &str) -> Result<Option<Offer>, EnvelopeError> {
        Ok(self
            .offers()
            .await?
            .into_iter()
            .find(|offer| offer.model == model))
    }

    /// Reserves a note, locks it against `seller`, and hands the session to
    /// the enclave sealed.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the wallet holds nothing, the lock
    /// does not land, or the enclave refuses the delegation.
    pub async fn open(
        &self,
        seller: Address,
        wanted: u64,
        window: u64,
    ) -> Result<Envelope, EnvelopeError> {
        if window == 0 || window > MAX_LOCK {
            return Err(EnvelopeError::Window(window));
        }
        let now = self.broadcast.timestamp().await?;
        let note = self.provision(wanted, now).await?;
        let envelope = Envelope {
            nullifier: note.nullifier,
            commitment: note.commitment,
            seller,
            deadline: now.saturating_add(window),
            ceiling: note.value,
        };
        let session = Session {
            nullifier: envelope.nullifier,
            seller,
            deadline: envelope.deadline,
        };

        let agreement = self.enclave.agree(&session).await?;
        let change_seed = Secret::random().map_err(|_| WalletError::Owner)?;
        // Before the lock, not after: a crash in between would leave a note
        // reserved whose change nothing could ever identify.
        self.envelopes
            .remember(envelope.nullifier, &change_seed)
            .await?;

        self.broadcast
            .submit(
                envelope.nullifier,
                self.terms
                    .pool
                    .lock(&session, agreement.signer, &agreement.lock),
            )
            .await?;
        if let Err(refused) = self.landed(envelope.nullifier, now).await {
            // A lock the chain rejected will never hold this note, so the
            // next session can have it now rather than after the hold lapses.
            if !matches!(refused, EnvelopeError::Abandoned) {
                self.wallet.release(note.commitment).await?;
            }
            return Err(refused);
        }

        self.deliver(&note, &envelope, &agreement, &change_seed, session)
            .await?;
        Ok(envelope)
    }

    /// Serves one request, opening or rolling an envelope if it has to.
    ///
    /// The envelope is the thing the agent never sees: a request that has
    /// nowhere to be billed opens one, a request that would overrun its
    /// ceiling or its deadline settles and opens the next, and either way the
    /// agent gets an answer rather than an error. Re-shopping is the same
    /// path — which seller the next envelope names comes from the catalogue,
    /// so a provider that got expensive or left is simply not chosen.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when a second attempt fails too, which means
    /// the trouble was not the envelope.
    pub async fn dispatch(
        &self,
        body: &[u8],
        attempt: &str,
    ) -> Result<BoxStream<'static, Result<Piece, EnclaveError>>, EnvelopeError> {
        let nullifier = match self.live().await? {
            Some(nullifier) => nullifier,
            None => self.roll(self.envelopes.newest().await?).await?,
        };
        match self.enclave.chat(nullifier, body, attempt).await {
            Ok(pieces) => Ok(pieces),
            // The envelope is spent, expiring, or gone. Replace it and ask
            // once more; a second failure was never about the envelope.
            Err(refused) if replaceable(&refused) => {
                let nullifier = self.roll(Some(nullifier)).await?;
                Ok(self.enclave.chat(nullifier, body, attempt).await?)
            }
            Err(refused) => Err(refused.into()),
        }
    }

    /// The envelope still serving, if one is.
    ///
    /// Derived rather than remembered: the registry says which nullifier was
    /// opened last, and the chain says whether it is still worth anything.
    async fn live(&self) -> Result<Option<Field>, EnvelopeError> {
        let Some(nullifier) = self.envelopes.newest().await? else {
            return Ok(None);
        };
        if self.index.is_spent(nullifier).await? {
            return Ok(None);
        }
        let now = self.broadcast.timestamp().await?;
        match self.wallet.escrow(nullifier).await? {
            Some(deadline) if now.saturating_add(BUFFER) < deadline => Ok(Some(nullifier)),
            _ => Ok(None),
        }
    }

    /// Replaces the envelope the caller found wanting.
    ///
    /// One at a time. An envelope runs out for everybody at once, and
    /// without this every request in flight would settle it and open its own
    /// replacement — a proof, a lock and a note each.
    ///
    /// `known` is what the caller was going on, and the check is against the
    /// registry rather than the chain: `open` writes there before the lock
    /// is even sent, so it answers while the chain is still catching up. A
    /// caller that queued behind someone else finds the registry has moved
    /// on, and takes what they made.
    async fn roll(&self, known: Option<Field>) -> Result<Field, EnvelopeError> {
        let _rolling = self.rolling.lock().await;
        if let Some(newest) = self.envelopes.newest().await?
            && Some(newest) != known
        {
            return Ok(newest);
        }
        if let Some(nullifier) = known {
            self.settle(nullifier, false).await?;
        }
        Ok(self
            .open(self.terms.seller, self.terms.session, self.terms.window)
            .await?
            .nullifier)
    }

    /// Settles an envelope and writes down the change it leaves.
    ///
    /// An envelope that served nothing still settles, for a unit: the pool
    /// cannot cancel a reservation, so the alternative is the buyer's capital
    /// sitting idle until the deadline.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the proof, the submission, or the
    /// recovery fails.
    pub async fn settle(&self, nullifier: Field, dust: bool) -> Result<(), EnvelopeError> {
        let Some(change_seed) = self.envelopes.change_seed(nullifier).await? else {
            return Err(EnvelopeError::Unknown);
        };
        let settlement = self.enclave.settle(nullifier, dust).await?;
        self.broadcast
            .submit(settlement.key, settlement.calldata)
            .await?;

        let now = self.broadcast.timestamp().await?;
        self.wallet
            .recover(nullifier, &change_seed, now)
            .await?;
        Ok(())
    }

    /// Writes down the change of a settlement we did not make.
    ///
    /// The seller closes a session itself when its deadline nears, so an
    /// envelope can be spent without this daemon having asked. The change
    /// is ours either way, and nothing else goes looking for it.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the chain or the store cannot be read.
    pub async fn recovered(&self) -> Result<(), EnvelopeError> {
        let Some(nullifier) = self.envelopes.newest().await? else {
            return Ok(());
        };
        if !self.index.is_spent(nullifier).await? {
            return Ok(());
        }
        let Some(change_seed) = self.envelopes.change_seed(nullifier).await? else {
            return Ok(());
        };
        let now = self.broadcast.timestamp().await?;
        self.wallet.recover(nullifier, &change_seed, now).await?;
        Ok(())
    }

    /// A note this envelope may delegate.
    ///
    /// A shielded note belongs to the wallet's own key, and handing that over
    /// would hand over every note it holds — so one that has no identity of
    /// its own is reseeded before it can be locked. A settlement mints its
    /// successor already seeded, so that cost falls on the first envelope
    /// rather than on each one.
    async fn provision(&self, wanted: u64, now: u64) -> Result<Held, EnvelopeError> {
        let note = self
            .wallet
            .reserve(self.terms.pool.asset(), wanted, now)
            .await?;
        if note.seed.is_some() {
            return Ok(note);
        }
        let witness = self.index.witness(&[note.leaf]).await?;
        let path = witness
            .paths
            .into_iter()
            .next()
            .ok_or(EnvelopeError::Unknown)?;
        let reseeded = self.wallet.reseed(&note, path, now).await?;
        let prepared = self.prover.transfer(reseeded.transaction).await?;
        self.broadcast
            .submit(reseeded.session, prepared.calldata)
            .await?;
        self.landed(reseeded.session, now).await?;
        self.placed(reseeded.session, now).await
    }

    /// Waits for the index to place a note the chain has already accepted.
    ///
    /// Landing and being indexed are different moments: the sync loop reads a
    /// block after it is mined, and no path can be built before it has.
    async fn placed(&self, commitment: Field, since: u64) -> Result<Held, EnvelopeError> {
        loop {
            let now = self.broadcast.timestamp().await?;
            self.wallet.reconcile(now).await?;
            if let Some(note) = self.wallet.by_commitment(commitment).await? {
                return Ok(note);
            }
            if now > since.saturating_add(PATIENCE) {
                return Err(EnvelopeError::Abandoned);
            }
            sleep(POLL).await;
        }
    }

    /// Waits for a submitted call to reach the chain.
    ///
    /// The lifecycle is advanced by the sync loop, so this only reads; a call
    /// that is replaced or dropped never landed, and whatever assumed it did
    /// has to be abandoned rather than retried onto a different note.
    async fn landed(&self, key: Field, since: u64) -> Result<(), EnvelopeError> {
        loop {
            match self.broadcast.landed(key).await? {
                Some(Landed::Confirmed { .. }) => return Ok(()),
                Some(Landed::Failed(reason)) => {
                    return Err(ChainError::Refused(reason).into());
                }
                Some(Landed::Dropped) => return Err(EnvelopeError::Abandoned),
                None if self.broadcast.timestamp().await? > since.saturating_add(PATIENCE) => {
                    return Err(EnvelopeError::Abandoned);
                }
                None => sleep(POLL).await,
            }
        }
    }

    /// Seals the delegation and hands it over.
    ///
    /// The path travels with it: the pool keeps every root it has reached, so
    /// one taken now never expires, and the enclave needs no chain access of
    /// its own.
    async fn deliver(
        &self,
        note: &Held,
        envelope: &Envelope,
        agreement: &Agreement,
        change_seed: &Secret,
        session: Session,
    ) -> Result<(), EnvelopeError> {
        let witness = self.index.witness(&[note.leaf]).await?;
        let path = witness.paths.first().ok_or(EnvelopeError::Unknown)?;
        let handover = Handover {
            seed: seed_of(note)?,
            value: note.value,
            asset: note.asset,
            leaf: note.leaf,
            siblings: path.siblings,
            root: witness.root,
            change_seed: Secret::from_bytes(change_seed.to_bytes())
                .map_err(|_| WalletError::Owner)?,
            seller: envelope.seller,
            deadline: envelope.deadline,
            pool: self.terms.pool.address(),
            chain_id: self.terms.pool.chain_id(),
            node: Address::ZERO,
            tee_signer: Address::ZERO,
        };
        let sealed = Recipient::accept(
            agreement.handover_key,
            &self.terms.pool,
            agreement.signer,
            &agreement.announcement,
        )
        .map_err(|_| EnclaveError::Refused("the enclave's key is not vouched for".into()))?
        .seal(&handover.to_bytes().map_err(|_| WalletError::Owner)?)
        .map_err(|_| EnclaveError::Refused("the delegation does not seal".into()))?;
        Ok(self.enclave.accept(sealed, session).await?)
    }
}

/// The seed a session note is delegated by. A note without one belongs to
/// the wallet's own key and must be split before it can be handed over.
fn seed_of(note: &Held) -> Result<Secret, EnvelopeError> {
    note.seed
        .as_ref()
        .map(|seed| Secret::from_bytes(seed.to_bytes()))
        .transpose()
        .map_err(|_| WalletError::Owner)?
        .ok_or(EnvelopeError::Unknown)
}

/// Whether a refusal is one a fresh envelope would fix.
const fn replaceable(refused: &EnclaveError) -> bool {
    matches!(
        refused,
        EnclaveError::CeilingReached | EnclaveError::Unknown
    )
}

/// Reads a whole answer out of the pieces it arrives in.
///
/// For callers that wanted one rather than a stream: a request that asked
/// for no streaming, and anything measuring what a session cost.
///
/// # Errors
///
/// Returns [`EnclaveError`] when a piece refuses, or the stream ends without
/// saying what the call cost.
pub async fn collected(
    mut pieces: BoxStream<'static, Result<Piece, EnclaveError>>,
) -> Result<Answered, EnclaveError> {
    let mut completion = Vec::new();
    let mut metered = None;
    while let Some(piece) = pieces.next().await {
        match piece? {
            Piece::Body(bytes) => completion.extend_from_slice(&bytes),
            Piece::Billed(bill) => metered = Some(bill),
        }
    }
    Ok(Answered {
        completion,
        metered: metered
            .ok_or_else(|| EnclaveError::Refused("an answer with no bill".into()))?,
    })
}

/// Mirrors `Pool.MAX_LOCK`: the pool refuses a longer reservation, and a
/// rejected lock costs the gas to find that out.
const MAX_LOCK: u64 = 3_600;
