use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{B256, keccak256},
    signers::{SignerSync, local::PrivateKeySigner},
};
use async_trait::async_trait;
use tokio::sync::RwLock;
use veil402_enclave::{Call, Meter, record as journal, sign_lock};
use veil402_sdk::{
    Field, Veil,
    evm::{PoolClient, Session},
    session::{Keyring, Sealed},
    wire::{AgreeBody, MeteredBody, SettlementBody, encode},
};

use crate::core::{Kept, MeterError, Metering, Meters, Replayed};

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
    async fn accept(&self, sealed: &Sealed, session: Session) -> Result<(), MeterError> {
        let key = self.keyring.handover(session.nullifier);
        let meter = Meter::open(self.pool, &key, sealed, session).map_err(refused)?;
        self.meters.write().await.insert(session.nullifier, meter);
        Ok(())
    }

    async fn record(
        &self,
        nullifier: Field,
        attempt: &str,
        price: u64,
        units: u64,
    ) -> Result<Metering, MeterError> {
        let mut meters = self.meters.write().await;
        let meter = meters.get_mut(&nullifier).ok_or(MeterError::Unknown)?;
        let sequence = meter.calls() as u64;
        let attestation = witnessed(nullifier, meter.calls(), units);
        let total = meter
            .record(Call {
                cost: price,
                attestation,
            })
            .map_err(|_| MeterError::CeilingReached)?;
        Ok(Metering {
            metered: MeteredBody {
                total,
                remaining: meter.ceiling().saturating_sub(total),
                attestation,
            },
            // Sealed to this enclave, so the host keeps it without reading it
            // and cannot hand back a total nobody served.
            record: journal(
                &self.keyring.handover(nullifier),
                sequence,
                attempt,
                total,
                attestation,
            )
            .map_err(refused)?,
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
                metered: MeteredBody {
                    total: record.total,
                    remaining: ceiling.saturating_sub(record.total),
                    attestation: record.attestation,
                },
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
