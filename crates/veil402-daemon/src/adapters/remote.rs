use async_trait::async_trait;
use reqwest::{Client, Response};
use veil402_sdk::{
    Field,
    evm::Session,
    session::Sealed,
    wire::{
        AcceptBody, Action, AgreeBody, CallBody, MeteredBody, Reply, SessionBody, SettlementBody,
        bytes, encode, field,
    },
};

use crate::core::envelope::{Agreement, Enclave, EnclaveError, Metered, Settlement};

/// Header naming the call a retry repeats, so one served twice is billed once.
const IDEMPOTENCY: &str = "idempotency-key";

fn unreachable(error: impl std::fmt::Display) -> EnclaveError {
    EnclaveError::Refused(error.to_string())
}

/// The enclave, reached over the gateway that hosts it.
///
/// HTTP/2 over one pooled connection: an envelope is thousands of small calls
/// against an inference latency that dwarfs them, so what matters is not
/// paying for a handshake each time.
pub struct RemoteEnclave {
    client: Client,
    base: String,
}

impl RemoteEnclave {
    /// # Errors
    ///
    /// Returns [`EnclaveError::Refused`] when the client cannot be built.
    pub fn new(base: &str) -> Result<Self, EnclaveError> {
        Ok(Self {
            client: Client::builder()
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .build()
                .map_err(unreachable)?,
            base: base.trim_end_matches('/').to_owned(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// Turns a refusal into the instruction it carries.
///
/// A gateway that answers with a problem body has already decided what the
/// daemon should do; anything else is a transport failure, which is always
/// worth another try.
async fn refusal(response: Response) -> EnclaveError {
    let status = response.status();
    let Ok(reply) = response.json::<Reply<serde_json::Value>>().await else {
        return EnclaveError::Refused(format!("the gateway answered {status}"));
    };
    let said = reply.error.unwrap_or_else(|| status.to_string());
    match reply.action {
        Some(Action::Retry) => EnclaveError::Unknown,
        // Paying is the daemon's own job, and it is already mid-way through
        // doing it: a 402 here means the gateway lost the session.
        Some(Action::Reshop | Action::Pay) => EnclaveError::CeilingReached,
        _ => EnclaveError::Refused(said),
    }
}

/// Unwraps the envelope every reply carries.
fn result<T>(reply: Reply<T>) -> Result<T, EnclaveError> {
    reply
        .result
        .ok_or_else(|| EnclaveError::Refused("the gateway answered with no result".into()))
}

#[async_trait]
impl Enclave for RemoteEnclave {
    async fn agree(&self, session: &Session) -> Result<Agreement, EnclaveError> {
        let response = self
            .client
            .post(self.url("/v1/sessions/agree"))
            .json(&SessionBody::new(session))
            .send()
            .await
            .map_err(unreachable)?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        let body: AgreeBody = result(response.json().await.map_err(unreachable)?)?;
        let handover_key: [u8; 32] = bytes(&body.handover_key)
            .map_err(unreachable)?
            .try_into()
            .map_err(|_| EnclaveError::Refused("the announced key is malformed".into()))?;
        Ok(Agreement {
            signer: body.signer,
            handover_key,
            announcement: bytes(&body.announcement).map_err(unreachable)?,
            lock: bytes(&body.lock).map_err(unreachable)?,
        })
    }

    async fn accept(&self, sealed: Sealed, session: Session) -> Result<(), EnclaveError> {
        let response = self
            .client
            .post(self.url("/v1/sessions/accept"))
            .json(&AcceptBody {
                session: SessionBody::new(&session),
                encapsulated: encode(&sealed.encapsulated),
                ciphertext: encode(&sealed.ciphertext),
            })
            .send()
            .await
            .map_err(unreachable)?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(refusal(response).await)
    }

    async fn record(
        &self,
        nullifier: Field,
        units: u64,
        attempt: &str,
    ) -> Result<Metered, EnclaveError> {
        let response = self
            .client
            .post(self.url(&format!(
                "/v1/sessions/{}/calls",
                encode(&nullifier.to_bytes())
            )))
            .header(IDEMPOTENCY, attempt)
            .json(&CallBody { units })
            .send()
            .await
            .map_err(unreachable)?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        let body: MeteredBody = result(response.json().await.map_err(unreachable)?)?;
        Ok(Metered {
            total: body.total,
            remaining: body.remaining,
            attestation: body.attestation,
        })
    }

    async fn settle(&self, nullifier: Field, dust: bool) -> Result<Settlement, EnclaveError> {
        let response = self
            .client
            .post(self.url(&format!(
                "/v1/sessions/{}/settle",
                encode(&nullifier.to_bytes())
            )))
            .query(&[("dust", dust)])
            .send()
            .await
            .map_err(unreachable)?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        let body: SettlementBody = result(response.json().await.map_err(unreachable)?)?;
        Ok(Settlement {
            key: field(&body.key).map_err(unreachable)?,
            calldata: bytes(&body.calldata).map_err(unreachable)?,
        })
    }
}
