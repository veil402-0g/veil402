use std::sync::Arc;

use thiserror::Error;
use veil402_sdk::{
    Field,
    evm::{PoolClient, Session},
    session::Sealed,
    wire::{AgreeBody, Challenge, MeteredBody, Offer, SettlementBody},
};

use alloy::primitives::{Address, B256, keccak256};

use super::{Escrows, Metering, Meters, Opened, Replay, Resumes, Served, Settlement};

/// How long before a deadline a settlement has to be under way.
///
/// Covers proving and inclusion. The pool allows no grace on this side: a
/// settlement a second late is refused outright, and the buyer keeps work
/// it did not pay for.
///
/// Deliberately shorter than the buyer's: this is the fallback for a buyer
/// that went away, so it must not pre-empt one that is still there. The gap
/// is the head start the buyer gets.
const BUFFER: u64 = 60;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MeterError {
    /// No session is held here. After a restart the delegation has to be
    /// handed over again before anything can be metered.
    #[error("the enclave does not hold this session")]
    Unknown,
    /// Serving this call would take the envelope past what its note can pay.
    #[error("the envelope's ceiling is reached")]
    CeilingReached,
    /// The pool holds no reservation this session could settle against, or
    /// holds one for somebody else.
    #[error("no escrow backs this session: {0}")]
    Unreserved(&'static str),
    /// This call has been served once already.
    ///
    /// The idempotency draft would have us replay the stored response, and
    /// a 409 reserved for one still in flight. Neither applies: the answer
    /// was sealed to a key the buyer mints per attempt, so a kept copy would
    /// not open for the retry — and keeping the plaintext instead is the one
    /// thing this design will not do. So the call is refused, and 409 says
    /// the key is spent rather than busy.
    #[error("that call was served already")]
    Repeated,
    /// The same key, a different request. The draft is unambiguous about
    /// this one, and it is a caller's bug rather than a race.
    #[error("that key was used for another request")]
    Reused,
    #[error("refused: {0}")]
    Refused(String),
    /// The seller's upstream refused, in its own words. The status is out
    /// here because it says nothing about the prompt; the words are sealed
    /// to the buyer, because they could.
    #[error("the upstream refused with {status}")]
    Upstream {
        status: u16,
        retry_after: Option<u64>,
        head: [u8; 32],
        sealed: Vec<u8>,
    },
}

/// A call under way, or what it would take to be allowed one.
pub enum Serving {
    /// Admitted and begun. The frames are sealed to the buyer; this side
    /// relays them without reading them.
    Streaming(Opened),
    /// No envelope covers this call. Not a failure — an answer.
    Owing(Challenge),
}

/// Prices what a buyer consumed and has it metered.
pub struct MeteringService {
    meters: Arc<dyn Meters>,
    served: Arc<dyn Served>,
    sessions: Arc<dyn Resumes>,
    escrows: Arc<dyn Escrows>,
    pool: PoolClient,
    /// Who the pool pays. One seller, so this is settled at boot rather
    /// than chosen per call.
    seller: Address,
    /// Measurements a buyer pins before it will pay. Empty until there is
    /// hardware to measure.
    pcrs: Vec<String>,
}

impl MeteringService {
    #[must_use]
    pub const fn new(
        meters: Arc<dyn Meters>,
        served: Arc<dyn Served>,
        sessions: Arc<dyn Resumes>,
        escrows: Arc<dyn Escrows>,
        pool: PoolClient,
        seller: Address,
        pcrs: Vec<String>,
    ) -> Self {
        Self {
            meters,
            served,
            sessions,
            escrows,
            pool,
            seller,
            pcrs,
        }
    }

    /// # Errors
    ///
    /// Returns [`MeterError`] when the upstream cannot be read.
    pub async fn offers(&self) -> Result<Vec<Offer>, MeterError> {
        self.meters.catalogue().await
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
        self.reserved(&session).await?;
        let accepted = self.meters.accept(sealed, session).await?;
        // The enclave proved the path reaches this root. Only a chain can say
        // the pool ever did — and without that, a buyer serves itself for
        // free out of a tree it invented.
        if !self.escrows.is_known_root(accepted.root).await? {
            self.meters.release(session.nullifier).await;
            return Err(MeterError::Unreserved("the pool never reached that root"));
        }
        // Kept before the first call, so a restart between the two still
        // finds a session rather than an empty log against nothing.
        self.sessions
            .opened(session.nullifier, sealed, session)
            .await
    }

    /// Checks the pool really holds this reservation, for us.
    async fn reserved(&self, session: &Session) -> Result<(), MeterError> {
        let escrow = self
            .escrows
            .escrow(session.nullifier)
            .await?
            .ok_or(MeterError::Unreserved("the pool holds no escrow"))?;
        if escrow.enclave != self.meters.signer() {
            return Err(MeterError::Unreserved("the escrow names another enclave"));
        }
        if escrow.seller != session.seller || escrow.deadline != session.deadline {
            return Err(MeterError::Unreserved("the escrow names another session"));
        }
        if escrow.settled {
            return Err(MeterError::Unreserved("the escrow is already settled"));
        }
        Ok(())
    }

    /// Serves a sealed request and meters what it consumed.
    ///
    /// The rate comes from the seller, never the buyer, and the quantity from
    /// the enclave, never the request: between them a buyer cannot write its
    /// own bill. `attempt` names the call rather than the retry, so one served
    /// twice is billed once.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when nothing is on offer, or the call would take
    /// the envelope past what its note can pay.
    pub async fn meter(
        &self,
        nullifier: Field,
        sealed: &Sealed,
        attempt: &str,
    ) -> Result<Serving, MeterError> {
        if !self.held(nullifier).await? {
            return Ok(Serving::Owing(self.challenge()));
        }
        // Served already. There is nothing kept to hand back, and making it
        // again would buy the same work twice — so this is a refusal, not a
        // second performance.
        if let Some(replay) = self.served.replayed(nullifier, attempt).await {
            return Err(if replay.request == asked(sealed) || replay.request.is_zero() {
                MeterError::Repeated
            } else {
                MeterError::Reused
            });
        }
        Ok(Serving::Streaming(
            self.meters.record(nullifier, attempt, sealed).await?,
        ))
    }

    /// Files what a finished call cost: the enclave's sealed record for a
    /// restart, and the bill itself so a repeat can be recognised.
    ///
    /// Separate from `meter` because the bill arrives at the end of a stream
    /// that has already left this process.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the record cannot be kept.
    pub async fn billed(
        &self,
        nullifier: Field,
        attempt: &str,
        sealed: &Sealed,
        metering: &Metering,
    ) -> Result<(), MeterError> {
        self.sessions.append(nullifier, &metering.record).await?;
        self.served
            .remember(
                nullifier,
                attempt,
                &Replay {
                    metered: metering.metered.clone(),
                    request: asked(sealed),
                },
            )
            .await;
        Ok(())
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

    /// Proves a settlement for every session near enough to its deadline
    /// that waiting for the buyer is no longer safe.
    ///
    /// The buyer settles when it wants its note back; one that has stopped
    /// caring never does, and the work was delivered either way. The pool
    /// refuses a settlement a second past the deadline, so the margin has
    /// to cover proving and inclusion both.
    ///
    /// Nothing is forgotten here. What was proved may still not reach the
    /// chain, and a session dropped on the way would never be tried again —
    /// so forgetting waits until something has actually been sent.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the chain or the store cannot be read.
    pub async fn closing(&self) -> Result<Vec<Settlement>, MeterError> {
        let now = self.escrows.now().await?;
        let mut due = Vec::new();
        for nullifier in self
            .sessions
            .expiring(now.saturating_add(BUFFER))
            .await?
        {
            // Nothing here can be settled, and trying again next pass would
            // only prove it again: the escrow is gone, somebody else closed
            // it, or the deadline went by while we were not looking.
            match self.escrows.escrow(nullifier).await? {
                Some(escrow) if !escrow.settled && escrow.deadline >= now => {}
                _ => {
                    self.sessions.forget(nullifier).await?;
                    continue;
                }
            }
            match self.meters.settle(nullifier, false).await {
                Ok(settlement) => due.push(Settlement {
                    nullifier,
                    calldata: veil402_sdk::wire::bytes(&settlement.calldata)
                        .map_err(|_| MeterError::Refused("a settlement that is not hex".into()))?,
                }),
                // Owed nothing, most likely. Its lock lapses on its own and
                // the note goes back; spending gas to hand it over sooner
                // buys the seller nothing.
                Err(error) => tracing::info!(%error, "a session was left to lapse"),
            }
        }
        Ok(due)
    }

    /// Lets go of sessions whose settlements are on their way.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the store cannot be written.
    pub async fn settled(&self, nullifiers: &[Field]) -> Result<(), MeterError> {
        for nullifier in nullifiers {
            self.sessions.forget(*nullifier).await?;
        }
        Ok(())
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
                .remember(
                    nullifier,
                    &replayed.attempt,
                    &Replay {
                        // A journal records what a session owed, never what
                        // was asked, so a key reused here cannot be told
                        // apart and is simply refused.
                        request: B256::ZERO,
                        metered: MeteredBody {
                            // The running total, not what one call added to
                            // it; nothing reads this one back.
                            cost: 0,
                            total: replayed.total,
                            remaining: replayed.remaining,
                            attestation: replayed.attestation,
                            // The stream is not replayed, only the bill.
                            delivered: None,
                            witness: None,
                            sequence: 0,
                        },
                    },
                )
                .await;
        }
        Ok(true)
    }

    fn challenge(&self) -> Challenge {
        Challenge {
            scheme: "escrow".to_owned(),
            pool: self.pool.address(),
            chain_id: self.pool.chain_id(),
            seller: self.seller,
            pcrs: self.pcrs.clone(),
        }
    }
}

/// Names a request without reading it, so one key used for two different
/// calls can be told from one used twice for the same.
fn asked(sealed: &Sealed) -> B256 {
    let mut bound = Vec::with_capacity(sealed.ciphertext.len() + 32);
    bound.extend_from_slice(&sealed.encapsulated);
    bound.extend_from_slice(&sealed.ciphertext);
    keccak256(bound)
}
