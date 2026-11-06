use std::sync::Arc;

use thiserror::Error;
use veil402_sdk::{
    Field,
    evm::{PoolClient, Session},
    session::Sealed,
    wire::{AgreeBody, Challenge, MeteredBody, Offer, SettlementBody},
};

use super::{Meters, Prices, Resumes, Served};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MeterError {
    /// No session is held here. After a restart the delegation has to be
    /// handed over again before anything can be metered.
    #[error("the enclave does not hold this session")]
    Unknown,
    /// Serving this call would take the envelope past what its note can pay.
    #[error("the envelope's ceiling is reached")]
    CeilingReached,
    /// This gateway has nothing to sell.
    #[error("no seller is offering anything here")]
    Empty,
    #[error("refused: {0}")]
    Refused(String),
}

/// A metered call, or what it would take to be allowed one.
#[derive(Clone, Debug)]
pub enum Serving {
    Metered(MeteredBody),
    /// No envelope covers this call. Not a failure — an answer.
    Owing(Challenge),
}

/// Prices what a buyer consumed and has it metered.
pub struct MeteringService {
    meters: Arc<dyn Meters>,
    prices: Arc<dyn Prices>,
    served: Arc<dyn Served>,
    sessions: Arc<dyn Resumes>,
    pool: PoolClient,
    /// Measurements a buyer pins before it will pay. Empty until there is
    /// hardware to measure.
    pcrs: Vec<String>,
}

impl MeteringService {
    #[must_use]
    pub const fn new(
        meters: Arc<dyn Meters>,
        prices: Arc<dyn Prices>,
        served: Arc<dyn Served>,
        sessions: Arc<dyn Resumes>,
        pool: PoolClient,
        pcrs: Vec<String>,
    ) -> Self {
        Self {
            meters,
            prices,
            served,
            sessions,
            pool,
            pcrs,
        }
    }

    #[must_use]
    pub fn offers(&self) -> &[Offer] {
        self.prices.offers()
    }

    /// # Errors
    ///
    /// Returns [`MeterError::Refused`] when signing fails.
    pub fn agree(&self, session: &Session) -> Result<AgreeBody, MeterError> {
        self.meters.agree(session)
    }

    /// # Errors
    ///
    /// Returns [`MeterError::Refused`] when the delegation does not open, or
    /// opens onto another note.
    pub async fn accept(&self, sealed: &Sealed, session: Session) -> Result<(), MeterError> {
        self.meters.accept(sealed, session).await?;
        // Kept before the first call, so a restart between the two still
        // finds a session rather than an empty log against nothing.
        self.sessions
            .opened(session.nullifier, sealed, session)
            .await
    }

    /// Prices units at the seller's rate and meters them.
    ///
    /// The rate comes from the seller, never the buyer: a buyer naming its own
    /// cost would be writing its own bill. `attempt` names the call rather
    /// than the retry, so one served twice is billed once.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when nothing is on offer, or the call would take
    /// the envelope past what its note can pay.
    pub async fn meter(
        &self,
        nullifier: Field,
        units: u64,
        attempt: &str,
    ) -> Result<Serving, MeterError> {
        let offer = self.prices.cheapest().ok_or(MeterError::Empty)?;
        if !self.held(nullifier).await? {
            return Ok(Serving::Owing(self.challenge(offer)));
        }
        if let Some(already) = self.served.replayed(nullifier, attempt).await {
            return Ok(Serving::Metered(already));
        }
        let metering = self
            .meters
            .record(nullifier, attempt, units.saturating_mul(offer.rate), units)
            .await?;
        self.sessions.append(nullifier, &metering.record).await?;
        self.served
            .remember(nullifier, attempt, &metering.metered)
            .await;
        Ok(Serving::Metered(metering.metered))
    }

    /// # Errors
    ///
    /// Returns [`MeterError::Unknown`] for a session this gateway does not
    /// hold, or [`MeterError::Refused`] when the proof cannot be built.
    pub async fn settle(&self, nullifier: Field, dust: bool) -> Result<SettlementBody, MeterError> {
        if !self.held(nullifier).await? {
            return Err(MeterError::Unknown);
        }
        let settlement = self.meters.settle(nullifier, dust).await?;
        // A settled session is nothing to anyone, so the store stops being a
        // record of who bought what.
        self.sessions.forget(nullifier).await?;
        Ok(settlement)
    }

    /// Whether the meter holds this session, picking it back up if a restart
    /// lost it and the host still has what it was handed.
    ///
    /// The buyer never learns a restart happened: it is a hiccup the host
    /// absorbs rather than an error the daemon has to handle.
    async fn held(&self, nullifier: Field) -> Result<bool, MeterError> {
        if self.meters.holds(nullifier).await {
            return Ok(true);
        }
        let Some(kept) = self.sessions.recall(nullifier).await? else {
            return Ok(false);
        };
        // What it already answered goes back too, or a retry that straddled
        // the restart would be metered a second time.
        for replayed in self.meters.resume(&kept).await? {
            self.served
                .remember(nullifier, &replayed.attempt, &replayed.metered)
                .await;
        }
        Ok(true)
    }

    fn challenge(&self, offer: &Offer) -> Challenge {
        Challenge {
            scheme: "escrow".to_owned(),
            pool: self.pool.address(),
            chain_id: self.pool.chain_id(),
            seller: offer.seller,
            rate: offer.rate,
            pcrs: self.pcrs.clone(),
        }
    }
}
