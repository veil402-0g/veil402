use async_trait::async_trait;
use std::sync::Arc;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Response};
use veil402_sdk::{
    Field,
    evm::{PoolClient, Session},
    session::Sealed,
    session::{Exchange, Inbound, Recipient},
    wire::{
        AcceptBody, Action, AgreeBody, CallBody, MeteredBody, Prompt, Reply, SessionBody,
        SettlementBody, bytes, encode, field,
    },
};

use crate::core::envelope::{Agreement, Answered, Enclave, EnclaveError, Metered, Settlement};

/// Header naming the call a retry repeats, so one served twice is billed once.
const IDEMPOTENCY: &str = "idempotency-key";

/// Named events on a relayed stream. Everything else is a sealed frame.
const HEAD: &str = "head";
const BILL: &str = "bill";
const ERROR: &str = "error";

/// What the gateway sends when it has nothing more to relay.
const DONE: &str = "[DONE]";

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
    /// The key each envelope's requests are sealed to, kept from `agree`.
    /// The gateway never gets a prompt in the clear.
    sealing: tokio::sync::RwLock<std::collections::HashMap<Field, Arc<Recipient>>>,
    pool: PoolClient,
}

impl RemoteEnclave {
    /// # Errors
    ///
    /// Returns [`EnclaveError::Refused`] when the client cannot be built.
    pub fn new(base: &str, pool: PoolClient) -> Result<Self, EnclaveError> {
        Ok(Self {
            client: Client::builder()
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .build()
                .map_err(unreachable)?,
            base: base.trim_end_matches('/').to_owned(),
            sealing: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            pool,
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
        // doing it: a 402 here means the envelope is spent or gone.
        Some(Action::Pay) => EnclaveError::CeilingReached,
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
        let announcement = bytes(&body.announcement).map_err(unreachable)?;
        // Checked here, not taken on trust: a host that could name the key
        // would be able to read every prompt it relays.
        let recipient = Recipient::accept(handover_key, &self.pool, body.signer, &announcement)
            .map_err(|_| EnclaveError::Refused("the announced key is not vouched for".into()))?;
        self.sealing
            .write()
            .await
            .insert(session.nullifier, Arc::new(recipient));
        Ok(Agreement {
            signer: body.signer,
            handover_key,
            announcement,
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

    async fn chat(
        &self,
        nullifier: Field,
        prompt: &Prompt,
        attempt: &str,
    ) -> Result<Answered, EnclaveError> {
        let Some(sealing) = self.sealing.read().await.get(&nullifier).cloned() else {
            return Err(EnclaveError::Unknown);
        };
        // A channel per request: the answer to one call cannot be read with
        // the key from another.
        let exchange = Exchange::open();
        let asked = Prompt {
            reply_to: exchange.public(),
            ..prompt.clone()
        };
        let sealed = sealing
            .seal_prompt(&borsh::to_vec(&asked).map_err(unreachable)?)
            .map_err(unreachable)?;

        let response = self
            .client
            .post(self.url(&format!(
                "/v1/sessions/{}/chat",
                encode(&nullifier.to_bytes())
            )))
            .header(IDEMPOTENCY, attempt)
            .json(&CallBody::new(&sealed))
            .send()
            .await
            .map_err(unreachable)?;
        // The status is the last thing the gateway can say plainly: after it,
        // everything is a frame.
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        collected(&exchange, response).await
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

/// Reads a relayed stream to its end and returns the whole answer.
///
/// The frames are sealed in sequence, so the gateway cannot reorder them or
/// slip one in; and the last is sealed as the last, so a relay that stops
/// early leaves the stream unfinished rather than looking complete.
async fn collected(exchange: &Exchange, response: Response) -> Result<Answered, EnclaveError> {
    let mut frames = response.bytes_stream().eventsource();
    let mut inbound = None;
    let mut answer = Vec::new();
    let mut metered = None;

    while let Some(event) = frames.next().await {
        let event = event.map_err(unreachable)?;
        match event.event.as_str() {
            HEAD => inbound = Some(opening(exchange, &event.data)?),
            BILL => {
                metered = Some(
                    serde_json::from_str::<MeteredBody>(&event.data).map_err(unreachable)?,
                );
            }
            ERROR => return Err(EnclaveError::Refused(event.data)),
            _ if event.data.trim() == DONE => break,
            _ => {
                let inbound = inbound
                    .as_mut()
                    .ok_or_else(|| EnclaveError::Refused("a frame before its key".into()))?;
                let sealed = bytes(&event.data).map_err(unreachable)?;
                // Which frame ended the answer is the enclave's to say, and
                // it said it inside the seal. Nothing between us gets a vote.
                answer.extend_from_slice(&inbound.frame(&sealed).map_err(unreachable)?.payload);
            }
        }
    }

    let inbound = inbound.ok_or_else(|| EnclaveError::Refused("a stream with no key".into()))?;
    if !inbound.done() {
        return Err(EnclaveError::Refused("the answer was cut short".into()));
    }
    let metered = metered.ok_or_else(|| EnclaveError::Refused("an answer with no bill".into()))?;
    Ok(Answered {
        completion: answer,
        metered: Metered {
            cost: metered.cost,
            total: metered.total,
            remaining: metered.remaining,
            attestation: metered.attestation,
        },
    })
}

fn opening(exchange: &Exchange, head: &str) -> Result<Inbound, EnclaveError> {
    let key: [u8; 32] = bytes(head)
        .map_err(unreachable)?
        .try_into()
        .map_err(|_| EnclaveError::Refused("a malformed stream key".into()))?;
    exchange.inbound(key).map_err(unreachable)
}
