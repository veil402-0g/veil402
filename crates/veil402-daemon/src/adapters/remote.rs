use async_trait::async_trait;
use std::sync::Arc;

use eventsource_stream::{Event, Eventsource, EventStreamError};
use futures_util::{StreamExt, stream, stream::BoxStream};
use reqwest::{Client, Response};
use veil402_sdk::{
    Field,
    evm::{PoolClient, Session},
    session::Sealed,
    session::{Exchange, Inbound, Recipient},
    wire::{
        AcceptBody, Action, AgreeBody, CallBody, MeteredBody, Offer, Prompt, RefusedBody, Reply,
        SessionBody, SettlementBody, bytes, encode, field,
    },
};

use crate::core::envelope::{Agreement, Enclave, EnclaveError, Metered, Piece, Settlement};

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
    match response.json::<Reply<serde_json::Value>>().await {
        Err(_) => EnclaveError::Refused(format!("the gateway answered {status}")),
        Ok(reply) => instructed(reply, status.as_u16()),
    }
}

fn instructed(reply: Reply<serde_json::Value>, status: u16) -> EnclaveError {
    let said = reply.error.unwrap_or_else(|| status.to_string());
    match reply.action {
        Some(Action::Retry) => EnclaveError::Unknown,
        // Paying is the daemon's own job, and it is already mid-way through
        // doing it: a 402 here means the envelope is spent or gone.
        Some(Action::Pay) => EnclaveError::CeilingReached,
        _ => EnclaveError::Refused(said),
    }
}

/// Opens the words an upstream refused with.
///
/// They were sealed to this call's own key before they left the enclave, so
/// a relay that kept a copy kept nothing it can read.
fn upstream(status: u16, refused: &RefusedBody, exchange: &Exchange) -> EnclaveError {
    let opened = bytes(&refused.head)
        .ok()
        .and_then(|head| <[u8; 32]>::try_from(head).ok())
        .and_then(|head| exchange.inbound(head).ok())
        .zip(bytes(&refused.sealed).ok())
        .and_then(|(mut inbound, sealed)| inbound.frame(&sealed).ok());
    opened.map_or_else(
        || EnclaveError::Refused(format!("the upstream refused with {status}, unreadably")),
        |frame| EnclaveError::Upstream {
            status,
            retry_after: refused.retry_after,
            body: frame.payload,
        },
    )
}

/// Unwraps the envelope every reply carries.
fn result<T>(reply: Reply<T>) -> Result<T, EnclaveError> {
    reply
        .result
        .ok_or_else(|| EnclaveError::Refused("the gateway answered with no result".into()))
}

#[async_trait]
impl Enclave for RemoteEnclave {
    async fn catalogue(&self) -> Result<Vec<Offer>, EnclaveError> {
        let response = self
            .client
            .get(self.url("/v1/catalogue"))
            .send()
            .await
            .map_err(unreachable)?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        result(response.json().await.map_err(unreachable)?)
    }

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
        body: &[u8],
        attempt: &str,
    ) -> Result<BoxStream<'static, Result<Piece, EnclaveError>>, EnclaveError> {
        let Some(sealing) = self.sealing.read().await.get(&nullifier).cloned() else {
            return Err(EnclaveError::Unknown);
        };
        // A channel per request: the answer to one call cannot be read with
        // the key from another.
        let exchange = Exchange::open();
        let asked = Prompt {
            body: body.to_vec(),
            reply_to: exchange.public(),
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
            let status = response.status().as_u16();
            let Ok(reply) = response.json::<Reply<serde_json::Value>>().await else {
                return Err(EnclaveError::Refused(format!(
                    "the gateway answered {status}"
                )));
            };
            return Err(reply
                .result
                .as_ref()
                .and_then(|carried| serde_json::from_value::<RefusedBody>(carried.clone()).ok())
                .map_or_else(
                    || instructed(reply.clone(), status),
                    |refused| upstream(status, &refused, &exchange),
                ));
        }
        Ok(reading(exchange, response))
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

/// What is left of a relayed stream, and what it takes to read it.
struct Reading {
    frames: BoxStream<'static, Result<Event, EventStreamError<reqwest::Error>>>,
    exchange: Exchange,
    inbound: Option<Inbound>,
    ended: bool,
}

impl Reading {
    /// The end of the stream, which is only an end if the sender said so.
    ///
    /// Every frame that arrived may have opened perfectly and the answer
    /// still be half of one: a relay that stops forwarding leaves exactly
    /// that. The last frame is sealed as the last, so its absence is the
    /// only thing that can say it.
    fn ending(mut self) -> Option<(Result<Piece, EnclaveError>, Self)> {
        if self.ended {
            return None;
        }
        self.ended = true;
        if self.inbound.as_ref().is_some_and(Inbound::done) {
            return None;
        }
        Some((
            Err(EnclaveError::Refused("the answer was cut short".into())),
            self,
        ))
    }
}

/// Reads a relayed stream, unsealing each frame as it arrives.
fn reading(
    exchange: Exchange,
    response: Response,
) -> BoxStream<'static, Result<Piece, EnclaveError>> {
    stream::unfold(
        Reading {
            frames: response.bytes_stream().eventsource().boxed(),
            exchange,
            inbound: None,
            ended: false,
        },
        |mut reading| async move {
            loop {
                let Some(event) = reading.frames.next().await else {
                    return reading.ending();
                };
                let event = match event {
                    Ok(event) => event,
                    Err(error) => return Some((Err(unreachable(error)), reading)),
                };
                match event.event.as_str() {
                    HEAD => match opening(&reading.exchange, &event.data) {
                        Ok(inbound) => reading.inbound = Some(inbound),
                        Err(refused) => return Some((Err(refused), reading)),
                    },
                    BILL => {
                        return Some((metered(&event.data), reading));
                    }
                    ERROR => {
                        return Some((Err(EnclaveError::Refused(event.data)), reading));
                    }
                    _ if event.data.trim() == DONE => return reading.ending(),
                    _ => {
                        let opened = match (reading.inbound.as_mut(), bytes(&event.data)) {
                            (Some(inbound), Ok(sealed)) => inbound.frame(&sealed),
                            (None, _) => {
                                return Some((
                                    Err(EnclaveError::Refused("a frame before its key".into())),
                                    reading,
                                ));
                            }
                            (_, Err(error)) => return Some((Err(unreachable(error)), reading)),
                        };
                        return Some((
                            opened
                                .map(|frame| Piece::Body(frame.payload))
                                .map_err(unreachable),
                            reading,
                        ));
                    }
                }
            }
        },
    )
    .boxed()
}

fn metered(data: &str) -> Result<Piece, EnclaveError> {
    let body: MeteredBody = serde_json::from_str(data).map_err(unreachable)?;
    Ok(Piece::Billed(Metered {
        cost: body.cost,
        total: body.total,
        remaining: body.remaining,
        attestation: body.attestation,
    }))
}

fn opening(exchange: &Exchange, head: &str) -> Result<Inbound, EnclaveError> {
    let key: [u8; 32] = bytes(head)
        .map_err(unreachable)?
        .try_into()
        .map_err(|_| EnclaveError::Refused("a malformed stream key".into()))?;
    exchange.inbound(key).map_err(unreachable)
}
