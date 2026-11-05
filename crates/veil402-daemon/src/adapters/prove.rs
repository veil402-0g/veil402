use std::sync::Arc;

use async_trait::async_trait;
use veil402_sdk::{
    Prepared, Transaction, Veil,
    evm::{PoolClient, Voucher},
};

use crate::core::envelope::{ProveError, Prover};

/// Proves through the local gnark worker.
///
/// Local because the witness holds the note secrets: a hosted prover would
/// see every one of them.
pub struct LocalProver {
    veil: Arc<Veil>,
    pool: PoolClient,
}

impl LocalProver {
    #[must_use]
    pub const fn new(veil: Arc<Veil>, pool: PoolClient) -> Self {
        Self { veil, pool }
    }
}

#[async_trait]
impl Prover for LocalProver {
    async fn transfer(&self, transaction: Transaction) -> Result<Prepared, ProveError> {
        self.veil
            .transfer(&self.pool, transaction)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "transfer could not be proved");
                ProveError::Failed
            })
    }

    async fn settle(
        &self,
        transaction: Transaction,
        voucher: Voucher,
    ) -> Result<Prepared, ProveError> {
        self.veil
            .settle(&self.pool, transaction, voucher)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "settlement could not be proved");
                ProveError::Failed
            })
    }
}
