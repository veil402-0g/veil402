use std::{collections::HashMap, sync::Arc};

use alloy::signers::{SignerSync, local::PrivateKeySigner};
use async_trait::async_trait;
use tokio::sync::RwLock;
use veil402_enclave::{Meter, sign_lock};
use veil402_sdk::{
    Field, Prepared, Veil,
    evm::{Call, PoolClient, Session},
    session::{Keyring, Sealed},
};

use crate::core::envelope::{Agreement, Enclave, EnclaveError, Metered};

fn refused(error: impl std::fmt::Display) -> EnclaveError {
    EnclaveError::Refused(error.to_string())
}

/// The enclave running in this process.
///
/// Everything it holds is derived or in memory, which is the contract real
/// hardware imposes anyway: a Nitro enclave has no disk, so a restart
/// re-derives its keys and is handed its sessions again. Swapping this file
/// for a vsock client is the whole of that move.
pub struct LocalEnclave {
    pool: PoolClient,
    keyring: Keyring,
    signer: PrivateKeySigner,
    veil: Arc<Veil>,
    meters: RwLock<HashMap<Field, Meter>>,
}

impl LocalEnclave {
    /// Derives the enclave's identity from its root seed.
    ///
    /// # Errors
    ///
    /// Returns [`EnclaveError::Refused`] when no signing key derives.
    pub fn new(pool: PoolClient, keyring: Keyring, veil: Arc<Veil>) -> Result<Self, EnclaveError> {
        let signer = keyring.signer().map_err(refused)?;
        Ok(Self {
            pool,
            keyring,
            signer,
            veil,
            meters: RwLock::new(HashMap::new()),
        })
    }
}

#[async_trait]
impl Enclave for LocalEnclave {
    async fn agree(&self, session: &Session) -> Result<Agreement, EnclaveError> {
        let key = self.keyring.handover(session.nullifier);
        let announcement = self
            .signer
            .sign_hash_sync(&key.announcement(&self.pool))
            .map_err(refused)?;
        Ok(Agreement {
            signer: self.signer.address(),
            handover_key: key.public(),
            announcement: announcement.as_bytes().to_vec(),
            lock: sign_lock(&self.pool, session, &self.signer).map_err(refused)?,
        })
    }

    async fn accept(&self, sealed: Sealed, session: Session) -> Result<(), EnclaveError> {
        let key = self.keyring.handover(session.nullifier);
        let meter = Meter::open(self.pool, &key, &sealed, session).map_err(refused)?;
        self.meters.write().await.insert(session.nullifier, meter);
        Ok(())
    }

    async fn record(&self, nullifier: Field, call: Call) -> Result<Metered, EnclaveError> {
        let mut meters = self.meters.write().await;
        let meter = meters.get_mut(&nullifier).ok_or(EnclaveError::Unknown)?;
        let total = meter
            .record(call)
            .map_err(|_| EnclaveError::CeilingReached)?;
        Ok(Metered {
            total,
            remaining: meter.ceiling().saturating_sub(total),
            attestation: call.attestation,
        })
    }

    async fn settle(&self, nullifier: Field, dust: bool) -> Result<Prepared, EnclaveError> {
        let meters = self.meters.read().await;
        let meter = meters.get(&nullifier).ok_or(EnclaveError::Unknown)?;
        if dust {
            meter.release(&self.veil, &self.signer).await
        } else {
            meter.settle(&self.veil, &self.signer).await
        }
        .map_err(refused)
    }
}
