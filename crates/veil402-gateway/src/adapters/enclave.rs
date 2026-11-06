use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{Address, B256, keccak256},
    signers::{SignerSync, local::PrivateKeySigner},
};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use futures_util::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::sync::RwLock;
use veil402_enclave::{
    Ask, Attested, Call, Meter, Receipt, Served, Upstream, UpstreamError, record as journal,
    sign_lock,
};
use veil402_sdk::{
    Field, Veil,
    evm::{PoolClient, Session},
    session::{EnclaveKey, Keyring, Outbound, Sealed},
    wire::{AgreeBody, MeteredBody, Offer, Prompt, SettlementBody, encode},
};

use crate::core::{
    Accepted, Billed, Kept, MeterError, Metering, Meters, Opened, Pricing, Replayed,
};

fn refused(error: impl std::fmt::Display) -> MeterError {
    MeterError::Refused(error.to_string())
}

/// The enclave, running in this process.
///
/// Holds nothing durable, which is the contract real hardware imposes anyway:
/// a Nitro enclave has no disk, so a restart re-derives its keys and is handed
/// its sessions again. The move to vsock replaces this file.
pub struct Enclave {
    pool: PoolClient,
    keyring: Keyring,
    signer: PrivateKeySigner,
    veil: Arc<Veil>,
    upstream: Arc<dyn Upstream>,
    /// Where the upstream's currency becomes the pool's, and the only place
    /// the two meet.
    pricing: Pricing,
    /// Shared because a call's bill lands after its stream has left this
    /// struct behind, and the stream has to outlive the borrow.
    meters: Arc<RwLock<HashMap<Field, Meter>>>,
}

impl Enclave {
    /// # Errors
    ///
    /// Returns [`MeterError::Refused`] when no signing key derives from the seed.
    pub fn new(
        pool: PoolClient,
        keyring: Keyring,
        veil: Arc<Veil>,
        upstream: Arc<dyn Upstream>,
        pricing: Pricing,
    ) -> Result<Self, MeterError> {
        let signer = keyring.signer().map_err(refused)?;
        Ok(Self {
            pool,
            keyring,
            signer,
            veil,
            upstream,
            pricing,
            meters: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// The address a buyer must find in the registry before it will pay.
    #[must_use]
    pub fn signer(&self) -> alloy::primitives::Address {
        self.signer.address()
    }
}

#[async_trait]
impl Meters for Enclave {
    fn signer(&self) -> Address {
        self.signer.address()
    }


    /// What this enclave can buy, filtered to what it can buy privately and
    /// priced the way this seller sells it.
    ///
    /// Anything attested only at the transport terminates TLS in an enclave
    /// and then hands the prompt to a vendor that reads it, which is not the
    /// thing being sold here.
    async fn catalogue(&self) -> Result<Vec<Offer>, MeterError> {
        Ok(self
            .upstream
            .catalogue()
            .await
            .map_err(declined)?
            .into_iter()
            .filter(|listing| listing.confidential)
            .map(|listing| Offer {
                model: listing.model,
                prompt: self.pricing.charge(listing.prompt),
                completion: self.pricing.charge(listing.completion),
                max_output: listing.max_output,
            })
            .collect())
    }

    fn agree(&self, session: &Session) -> Result<AgreeBody, MeterError> {
        let key = self.keyring.handover(session.nullifier);
        let announcement = self
            .signer
            .sign_hash_sync(&key.announcement(&self.pool))
            .map_err(refused)?;
        Ok(AgreeBody {
            signer: self.signer.address(),
            handover_key: encode(&key.public()),
            announcement: encode(&announcement.as_bytes()),
            lock: encode(&sign_lock(&self.pool, session, &self.signer).map_err(refused)?),
        })
    }

    /// Takes a delegation, after checking it against the escrow it names.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError::Refused`] when the envelope does not open, or opens
    /// onto another note.
    async fn accept(&self, sealed: &Sealed, session: Session) -> Result<Accepted, MeterError> {
        let key = self.keyring.handover(session.nullifier);
        let meter = Meter::open(self.pool, &key, sealed, session).map_err(refused)?;
        let accepted = Accepted { root: meter.root() };
        self.meters.write().await.insert(session.nullifier, meter);
        Ok(accepted)
    }

    async fn release(&self, nullifier: Field) {
        self.meters.write().await.remove(&nullifier);
    }

    async fn record(
        &self,
        nullifier: Field,
        attempt: &str,
        sealed: &Sealed,
    ) -> Result<Opened, MeterError> {
        let prompt = opened(&self.keyring.handover(nullifier), sealed)?;
        let budget = {
            let meters = self.meters.read().await;
            let meter = meters.get(&nullifier).ok_or(MeterError::Unknown)?;
            self.pricing.affords(meter.headroom())
        };
        // Asked for before a byte is sent, and refused here rather than
        // part-way through: an upstream prices a call once it has served it,
        // so there is no later moment to stop at.
        let frames = match self
            .upstream
            .ask(Ask {
                model: model(&prompt.body)?,
                body: prompt.body,
                budget,
            })
            .await
        {
            Ok(frames) => frames,
            // A refusal is an answer of a kind, and the buyer is the only
            // one entitled to read it.
            Err(UpstreamError::Refused {
                status,
                body,
                retry_after,
            }) => {
                let mut outbound = EnclaveKey::reply_stream(prompt.reply_to).map_err(refused)?;
                let head = outbound.head();
                return Err(MeterError::Upstream {
                    status,
                    retry_after,
                    head,
                    sealed: outbound.frame(&body, true).map_err(refused)?,
                });
            }
            Err(other) => return Err(declined(other)),
        };

        let outbound = EnclaveKey::reply_stream(prompt.reply_to).map_err(refused)?;
        let head = outbound.head();
        Ok(Opened {
            head,
            frames: relaying(Relay {
                frames,
                outbound,
                pending: None,
                meters: self.meters.clone(),
                key: self.keyring.handover(nullifier),
                nullifier,
                attempt: attempt.to_owned(),
                pricing: self.pricing,
            }),
        })
    }

    async fn resume(&self, kept: &Kept) -> Result<Vec<Replayed>, MeterError> {
        let nullifier = kept.session.nullifier;
        let (meter, recorded) = Meter::resume(
            self.pool,
            &self.keyring.handover(nullifier),
            &kept.sealed,
            kept.session,
            &kept.log,
        )
        .map_err(refused)?;
        let ceiling = meter.ceiling();
        tracing::info!(calls = meter.calls(), "a session was picked back up");
        self.meters.write().await.insert(nullifier, meter);
        Ok(recorded
            .into_iter()
            // A call nobody named cannot be recognised on a retry, so there
            // is nothing to restore for it.
            .filter(|record| !record.attempt.is_empty())
            .map(|record| Replayed {
                total: record.total,
                remaining: ceiling.saturating_sub(record.total),
                attestation: record.attestation,
                attempt: record.attempt,
            })
            .collect())
    }

    /// Proves the settlement. `dust` asks for the smallest one that frees the
    /// note, for an envelope nobody owes anything on.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError::Unknown`] for a session this enclave does not hold,
    /// or [`Error::Refused`] when the proof cannot be built.
    async fn settle(&self, nullifier: Field, dust: bool) -> Result<SettlementBody, MeterError> {
        // What a settlement pays is fixed the moment it is built, so the
        // session stops metering first: a call counted after this is one the
        // settlement cannot cover.
        self.meters
            .write()
            .await
            .get_mut(&nullifier)
            .ok_or(MeterError::Unknown)?
            .closing();
        let meters = self.meters.read().await;
        let meter = meters.get(&nullifier).ok_or(MeterError::Unknown)?;
        let prepared = if dust {
            meter.release(&self.veil, &self.signer).await
        } else {
            meter.settle(&self.veil, &self.signer).await
        }
        .map_err(refused)?;
        tracing::debug!(root = ?prepared.proof.public.root, "settlement proved");
        Ok(SettlementBody {
            // An output rather than the nullifier: the lock was tracked under
            // that already, and a submission is idempotent on its key.
            key: encode(&prepared.proof.public.commitments[0].to_bytes()),
            calldata: encode(&prepared.calldata),
        })
    }

    async fn holds(&self, nullifier: Field) -> bool {
        self.meters.read().await.contains_key(&nullifier)
    }
}

/// What the enclave is relaying, and everything the relay outlives it with.
struct Relay {
    frames: BoxStream<'static, Result<Served, UpstreamError>>,
    outbound: Outbound,
    /// The bill, held back one step so the sealed stream can be closed first.
    pending: Option<Metering>,
    meters: Arc<RwLock<HashMap<Field, Meter>>>,
    key: EnclaveKey,
    nullifier: Field,
    attempt: String,
    pricing: Pricing,
}

/// Seals each frame on its way past, and bills the call when it ends.
fn relaying(relay: Relay) -> BoxStream<'static, Result<Billed, MeterError>> {
    stream::unfold(relay, |mut relay| async move {
        if let Some(metering) = relay.pending.take() {
            return Some((Ok(Billed::Bill(metering)), relay));
        }
        let served = match relay.frames.next().await? {
            Ok(served) => served,
            Err(error) => return Some((Err(declined(error)), relay)),
        };
        let next = match served {
            Served::Frame(bytes) => relay
                .outbound
                .frame(&bytes, false)
                .map(Billed::Frame)
                .map_err(refused),
            // A frame of its own, sealed as the last, so a relay that stops
            // early cannot pass a half-answer off as a whole one. Nothing
            // out here can tell it from any other frame, which is the point.
            Served::Done(receipt) => match bill(&mut relay, &receipt).await {
                Ok(metering) => {
                    relay.pending = Some(metering);
                    relay.outbound.frame(&[], true).map(Billed::Frame).map_err(refused)
                }
                Err(error) => Err(error),
            },
        };
        Some((next, relay))
    })
    .boxed()
}

/// Prices what the upstream charged, records it, and seals the record.
async fn bill(relay: &mut Relay, receipt: &Receipt) -> Result<Metering, MeterError> {
    let mut meters = relay.meters.write().await;
    let meter = meters.get_mut(&relay.nullifier).ok_or(MeterError::Unknown)?;
    // Clamped rather than refused: the work is done and the bytes have gone,
    // and a settlement larger than its note could not be proved.
    //
    // The clamp is a backstop, not a budget. Admission authorises what the
    // call may cost; a capture above that is the seller paying the
    // difference, so one that fires is worth knowing about rather than
    // absorbing quietly.
    let owed = relay.pricing.charge(receipt.cost);
    let cost = owed.min(meter.headroom());
    if cost < owed {
        tracing::warn!(
            owed,
            billed = cost,
            "a call cost more than it was admitted for"
        );
    }
    // A provider that offered a signature which did not recover is a
    // different thing from one that offered none, and worth saying so.
    if receipt.attested == Attested::Failed {
        tracing::warn!(
            provider = ?receipt.provider,
            "a provider's signature did not match its attested key"
        );
    }
    let attestation = witnessed(receipt);
    let sequence = meter.calls() as u64;
    let total = meter
        .record(Call { cost, attestation })
        .map_err(|_| MeterError::CeilingReached)?;
    Ok(Metering {
        metered: MeteredBody {
            cost,
            total,
            remaining: meter.ceiling().saturating_sub(total),
            attestation,
        },
        record: journal(&relay.key, sequence, &relay.attempt, total, attestation)
            .map_err(refused)?,
    })
}

fn opened(key: &EnclaveKey, sealed: &Sealed) -> Result<Prompt, MeterError> {
    let bytes = key.open_prompt(sealed).map_err(refused)?;
    Prompt::try_from_slice(&bytes)
        .map_err(|_| MeterError::Refused("the request does not decode".into()))
}

/// Which model the request names. Read from the body rather than carried
/// beside it, so the one we price is the one the upstream serves.
fn model(body: &[u8]) -> Result<String, MeterError> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|request| Some(request.get("model")?.as_str()?.to_owned()))
        .ok_or(MeterError::Refused("the request names no model".into()))
}

/// Binds the upstream's own receipt, so what a settlement covers can be
/// matched against what the provider says it served.
fn witnessed(receipt: &Receipt) -> B256 {
    let mut bound = Vec::with_capacity(96);
    bound.extend_from_slice(receipt.reference.as_bytes());
    bound.extend_from_slice(&receipt.cost.to_be_bytes());
    bound.extend_from_slice(&receipt.tokens.prompt.to_be_bytes());
    bound.extend_from_slice(&receipt.tokens.completion.to_be_bytes());
    if let Some(provider) = receipt.provider {
        bound.extend_from_slice(provider.as_slice());
    }
    keccak256(bound)
}

/// An upstream refusal, as the question the gateway has to answer: can this
/// be tried again, somewhere else, or not at all.
fn declined(error: UpstreamError) -> MeterError {
    match error {
        UpstreamError::Unaffordable => MeterError::CeilingReached,
        other => MeterError::Refused(other.to_string()),
    }
}
