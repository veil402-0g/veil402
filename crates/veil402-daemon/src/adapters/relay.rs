//! Sending pool calls through a key that is not ours.
//!
//! Lock and settle answer to a proof or the enclave's signature, never to
//! whoever sent them, so there is nothing to gain by sending them ourselves
//! — and an address that signs both is an address that links them. The
//! deposit still goes under our own key: `shield` pulls tokens from the
//! caller, so no one else can make it.

use alloy::{
    eips::BlockId,
    providers::Provider,
};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use veil402_sdk::Field;

use crate::core::envelope::{Broadcaster, ChainError, Landed};

#[derive(Serialize)]
struct Submission<'a> {
    key: String,
    calldata: &'a str,
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum Reached {
    Unknown,
    Pending,
    Confirmed { block: u64 },
    Failed { reason: String },
    Dropped,
}

/// Submits through a relay and reads the chain directly.
///
/// Reading is not the part that links anything: the pool's state is public
/// and asking for it says nothing about who holds which note.
pub struct RelayBroadcaster<P> {
    http: Client,
    base: String,
    provider: P,
}

fn unreachable(base: &str) -> ChainError {
    ChainError::Refused(format!("the relay at {base} did not answer"))
}

impl<P> RelayBroadcaster<P> {
    /// Opens a broadcaster against a running relay.
    ///
    /// The relay is asked to answer before anything depends on it. A daemon
    /// that silently fell back to its own key would keep working and quietly
    /// stop being private, which is the one failure worth refusing to start
    /// over.
    ///
    /// # Errors
    ///
    /// Returns [`ChainError`] when the relay cannot be reached.
    pub async fn open(base: &str, provider: P) -> Result<Self, ChainError> {
        let base = base.trim_end_matches('/').to_owned();
        let http = Client::new();
        let answered = http
            .get(format!("{base}/healthz"))
            .send()
            .await
            .map_err(|_| unreachable(&base))?;
        if !answered.status().is_success() {
            return Err(unreachable(&base));
        }
        Ok(Self {
            http,
            base,
            provider,
        })
    }
}

#[async_trait]
impl<P: Provider + Send + Sync> Broadcaster for RelayBroadcaster<P> {
    async fn submit(&self, key: Field, calldata: Vec<u8>) -> Result<(), ChainError> {
        let answered = self
            .http
            .post(format!("{}/v1/submit", self.base))
            .json(&Submission {
                key: hex::encode(key.to_bytes()),
                calldata: &hex::encode(&calldata),
            })
            .send()
            .await
            .map_err(|_| unreachable(&self.base))?;
        match answered.status() {
            StatusCode::ACCEPTED | StatusCode::OK => Ok(()),
            // The relay will not sign for this call whatever we do, so
            // retrying it only wastes the attempts a settlement has left.
            StatusCode::FORBIDDEN => Err(ChainError::Refused(
                "the relay does not send that call".into(),
            )),
            status => {
                let body = answered.text().await.unwrap_or_default();
                tracing::warn!(%status, body = %body.chars().take(300).collect::<String>(),
                    "the relay would not take the call");
                Err(ChainError::Unavailable)
            }
        }
    }

    async fn landed(&self, key: Field) -> Result<Option<Landed>, ChainError> {
        let answered = self
            .http
            .get(format!("{}/v1/submit/{}", self.base, hex::encode(key.to_bytes())))
            .send()
            .await
            .map_err(|_| unreachable(&self.base))?;
        if !answered.status().is_success() {
            let status = answered.status();
            let body = answered.text().await.unwrap_or_default();
            tracing::warn!(%status, body = %body.chars().take(300).collect::<String>(),
                "the relay would not say what landed");
            return Err(ChainError::Unavailable);
        }
        Ok(
            match answered
                .json::<Reached>()
                .await
                .inspect_err(|error| tracing::warn!(%error, "the relay answered in a shape we do not read"))
                .map_err(|_| ChainError::Unavailable)?
            {
                Reached::Confirmed { block } => Some(Landed::Confirmed { block }),
                Reached::Failed { reason } => Some(Landed::Failed(reason)),
                Reached::Dropped => Some(Landed::Dropped),
                Reached::Unknown | Reached::Pending => None,
            },
        )
    }

    async fn timestamp(&self) -> Result<u64, ChainError> {
        self.provider
            .get_block(BlockId::latest())
            .await
            .inspect_err(|error| tracing::warn!(%error, "the head block is unreadable"))
            .map_err(|_| ChainError::Unavailable)?
            .map(|block| block.header.timestamp)
            .ok_or_else(|| {
                tracing::warn!("the endpoint answered without a latest block");
                ChainError::Unavailable
            })
    }
}
