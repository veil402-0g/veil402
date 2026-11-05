use std::sync::Arc;

use alloy::primitives::Address;
use thiserror::Error;
use veil402_sdk::{
    Field, Secret,
    evm::{Call, PoolClient, Session},
    session::{Handover, Recipient},
};

use super::{
    Agreement, Broadcaster, ChainError, Enclave, EnclaveError, Envelope, EnvelopeStore, Landed,
    Metered, Phase, ProveError, Prover,
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
}

/// What shapes an envelope, beyond the ports it runs over.
#[derive(Clone, Copy, Debug)]
pub struct Terms {
    pub pool: PoolClient,
    /// Where the remainder of a settlement returns.
    pub change_owner: Field,
    /// Size a settlement should leave behind for the next envelope.
    pub target: u64,
}

impl EnvelopeService {
    #[must_use]
    pub const fn new(
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
        }
    }

    /// Reserves a note, locks it against `seller`, and hands the session to
    /// the enclave sealed.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when no note covers the ceiling, the lock
    /// does not land, or the enclave refuses the delegation.
    pub async fn open(
        &self,
        seller: Address,
        ceiling: u64,
        window: u64,
    ) -> Result<Envelope, EnvelopeError> {
        if window == 0 || window > MAX_LOCK {
            return Err(EnvelopeError::Window(window));
        }
        let now = self.broadcast.timestamp().await?;
        let note = self.provision(ceiling, now).await?;
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

    /// Meters one served call.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError::Enclave`] when the ceiling is reached or the
    /// enclave no longer holds the session.
    pub async fn record(&self, nullifier: Field, call: Call) -> Result<Metered, EnvelopeError> {
        Ok(self.enclave.record(nullifier, call).await?)
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
        let prepared = self.enclave.settle(nullifier, dust).await?;
        // Keyed by an output rather than the nullifier: the lock already used
        // that key, and submission is idempotent on it.
        let key = prepared.proof.public.commitments[0];
        self.broadcast.submit(key, prepared.calldata).await?;

        let now = self.broadcast.timestamp().await?;
        self.wallet
            .recover(nullifier, &change_seed, self.terms.target, now)
            .await?;
        Ok(())
    }

    /// Where an envelope stands, read from the chain.
    ///
    /// # Errors
    ///
    /// Returns [`EnvelopeError`] when the index cannot be read.
    pub async fn phase(&self, envelope: &Envelope, now: u64) -> Result<Phase, EnvelopeError> {
        if self.index.is_spent(envelope.nullifier).await? {
            return Ok(Phase::Settled);
        }
        match self.wallet.escrow(envelope.nullifier).await? {
            None => Ok(Phase::Locking),
            Some(deadline) if now.saturating_add(BUFFER) >= deadline => Ok(Phase::Expiring),
            Some(_) => Ok(Phase::Open),
        }
    }

    /// A note this envelope may delegate.
    ///
    /// A shielded note belongs to the wallet's own key, and handing that over
    /// would hand over every note it holds, so a cold note is split into one
    /// a session owns before it can be locked. Settlements mint the next one
    /// already seeded, so this happens once per model rather than per
    /// envelope.
    async fn provision(&self, ceiling: u64, now: u64) -> Result<Held, EnvelopeError> {
        let note = self
            .wallet
            .reserve(self.terms.pool.asset(), ceiling, now)
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
        let split = self.wallet.split(&note, path, ceiling, now).await?;
        let prepared = self.prover.transfer(split.transaction).await?;
        self.broadcast
            .submit(split.session, prepared.calldata)
            .await?;
        self.landed(split.session, now).await?;
        self.wallet.reconcile(now).await?;
        self.wallet
            .by_commitment(split.session)
            .await?
            .ok_or(EnvelopeError::Abandoned)
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
                None => tokio::task::yield_now().await,
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
            change_owner: self.terms.change_owner,
            change_seed: Secret::from_bytes(change_seed.to_bytes())
                .map_err(|_| WalletError::Owner)?,
            target: self.terms.target,
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

/// Mirrors `Pool.MAX_LOCK`: the pool refuses a longer reservation, and a
/// rejected lock costs the gas to find that out.
const MAX_LOCK: u64 = 3_600;
