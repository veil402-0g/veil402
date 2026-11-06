use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{Address, B256, keccak256},
    signers::{SignerSync, local::PrivateKeySigner},
};
use async_trait::async_trait;
use tokio::sync::RwLock;
use borsh::BorshDeserialize;
use veil402_enclave::{Call, Meter, record as journal, sign_lock};
use veil402_sdk::{
    Field, Veil,
    evm::{PoolClient, Session},
    session::{EnclaveKey, Keyring, Sealed},
    wire::{AgreeBody, Completion, MeteredBody, Prompt, SettlementBody, encode},
};

use crate::core::{Accepted, Kept, MeterError, Metering, Meters, Replayed};

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
    meters: RwLock<HashMap<Field, Meter>>,
}

impl Enclave {
    /// # Errors
    ///
    /// Returns [`MeterError::Refused`] when no signing key derives from the seed.
    pub fn new(pool: PoolClient, keyring: Keyring, veil: Arc<Veil>) -> Result<Self, MeterError> {
        let signer = keyring.signer().map_err(refused)?;
        Ok(Self {
            pool,
            keyring,
            signer,
            veil,
            meters: RwLock::new(HashMap::new()),
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

    async fn serve(&self, nullifier: Field, sealed: &Sealed) -> Result<Sealed, MeterError> {
        let key = self.keyring.handover(nullifier);
        let prompt = opened(&key, sealed)?;
        let body = borsh::to_vec(&serve(&prompt))
            .map_err(|_| MeterError::Refused("the answer does not encode".into()))?;
        EnclaveKey::reply(prompt.reply_to, &body).map_err(refused)
    }

    async fn record(
        &self,
        nullifier: Field,
        attempt: &str,
        rate: u64,
        sealed: &Sealed,
    ) -> Result<Metering, MeterError> {
        let key = self.keyring.handover(nullifier);
        let prompt = opened(&key, sealed)?;
        let answer = serve(&prompt);
        // Counted here because here is the only place the request exists in
        // the clear. A length the buyer or the provider reported would be a
        // number neither side can check.
        let units = consumed(&prompt, &answer);

        let mut meters = self.meters.write().await;
        let meter = meters.get_mut(&nullifier).ok_or(MeterError::Unknown)?;
        let sequence = meter.calls() as u64;
        let attestation = witnessed(nullifier, meter.calls(), units);
        let total = meter
            .record(Call {
                cost: units.saturating_mul(rate),
                attestation,
            })
            .map_err(|_| MeterError::CeilingReached)?;
        let body = borsh::to_vec(&answer)
            .map_err(|_| MeterError::Refused("the answer does not encode".into()))?;
        Ok(Metering {
            completion: EnclaveKey::reply(prompt.reply_to, &body).map_err(refused)?,
            metered: MeteredBody {
                total,
                remaining: meter.ceiling().saturating_sub(total),
                attestation,
            },
            // Sealed to this enclave, so the host keeps it without reading it
            // and cannot hand back a total nobody served.
            record: journal(&key, sequence, attempt, total, attestation).map_err(refused)?,
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
        let meters = self.meters.read().await;
        let meter = meters.get(&nullifier).ok_or(MeterError::Unknown)?;
        let prepared = if dust {
            meter.release(&self.veil, &self.signer).await
        } else {
            meter.settle(&self.veil, &self.signer).await
        }
        .map_err(refused)?;
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

fn opened(key: &EnclaveKey, sealed: &Sealed) -> Result<Prompt, MeterError> {
    let bytes = key.open_prompt(sealed).map_err(refused)?;
    Prompt::try_from_slice(&bytes)
        .map_err(|_| MeterError::Refused("the request does not decode".into()))
}

/// What a provider would return, once there is one. Labelled rather than
/// echoed: a stub that looked like a model's answer would eventually be
/// mistaken for one.
fn serve(prompt: &Prompt) -> Completion {
    Completion {
        model: prompt.model.clone(),
        content: "[veil402: no provider configured]".to_owned(),
    }
}

/// What the exchange consumed, in the units a seller prices.
fn consumed(prompt: &Prompt, answer: &Completion) -> u64 {
    let asked: usize = prompt
        .messages
        .iter()
        .map(|message| message.content.len())
        .sum();
    (asked + answer.content.len()) as u64
}

/// Stands in for the digest of a request, its response, and the provider's
/// attestation over both. Until inference is real there is nothing to bind,
/// and a placeholder that pretends otherwise would be worse than an obvious
/// one.
fn witnessed(nullifier: Field, sequence: usize, units: u64) -> B256 {
    let mut bound = Vec::with_capacity(48);
    bound.extend_from_slice(&nullifier.to_bytes());
    bound.extend_from_slice(&(sequence as u64).to_be_bytes());
    bound.extend_from_slice(&units.to_be_bytes());
    keccak256(bound)
}
