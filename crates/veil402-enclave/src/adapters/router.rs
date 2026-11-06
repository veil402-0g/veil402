//! The 0G Compute Router: one key, one endpoint, every model it fronts.
//!
//! It routes and bills without being able to forge a reply, but it does read
//! the request — so this adapter buys inference, not privacy. Sealing the
//! body to the provider's own enclave is a second adapter beside this one,
//! and the port is shaped so nothing above has to know which is in use.

use std::{sync::Arc, time::Duration};

use alloy::primitives::{Address, Signature, eip191_hash_message};
use async_trait::async_trait;
use eventsource_stream::{Event, Eventsource, EventStreamError};
use futures_util::{StreamExt, stream, stream::BoxStream};
use reqwest::{Client, Response};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::core::ports::{
    Ask, Attested, Listing, Receipt, Registry, Served, Tokens, Upstream, UpstreamError,
};

/// Names the signature a provider holds for a call it served.
const RESPONSE_KEY: &str = "zg-res-key";

/// Where a router reports what a call cost.
const TRACE: &str = "x_0g_trace";

/// Ends a stream, by convention rather than by framing.
const DONE: &str = "[DONE]";

/// Models whose weights run inside an enclave. The other attested kind
/// terminates TLS in one and proxies to a vendor that reads the prompt, so
/// only this one can carry the claim.
const CONFIDENTIAL: &str = "TeeML";

const CHAT: &str = "chatbot";

/// Asks for a provider that seals rather than one that merely attests.
const TRUST: &str = "X-0G-Provider-Trust-Mode";

/// A price is quoted in USD per token; a bill arrives in micro-USD. Twelve
/// decimal places carries one into the other: a million tokens, a million
/// micro-dollars.
const SCALE: u32 = 12;

/// Long enough that a connection survives the gap between calls in a
/// session, which is where the handshake would otherwise be paid again.
const IDLE: Duration = Duration::from_secs(90);

/// How long a provider may take to say what it signed.
///
/// Short, and far below [`STALL`]: the answer is already out, but the bill
/// rides on the same frame, so a provider that hangs here would hold up
/// settlement rather than delivery. Measured round trip is under half a
/// second.
const VOUCH: Duration = Duration::from_secs(5);

/// How long a stream may say nothing at all before it is abandoned.
///
/// Per read rather than per request: a whole-request timeout would cut off
/// a long answer that was arriving perfectly well. A provider that simply
/// stops would otherwise hold the envelope's budget, and the task draining
/// it, for as long as it liked.
const STALL: Duration = Duration::from_mins(1);

pub struct Router {
    client: Client,
    base: String,
    key: String,
    trust: String,
    /// What a call may spend when the request names no limit of its own.
    ceiling: u32,
    /// Prices, as of the first call that needed them.
    catalogue: RwLock<Vec<Listing>>,
    /// Where a provider's attested key and endpoint are looked up.
    vouched: Arc<dyn Registry>,
}

impl Router {
    /// # Errors
    ///
    /// Returns [`UpstreamError::Unavailable`] when the client cannot be built.
    pub fn new(
        base: &str,
        key: &str,
        trust: &str,
        ceiling: u32,
        vouched: Arc<dyn Registry>,
    ) -> Result<Self, UpstreamError> {
        Self::via(base, key, trust, ceiling, vouched, None)
    }

    /// The same upstream, reached through a local hop.
    ///
    /// Only the address is overridden. The client still speaks to the real
    /// hostname and still checks the real certificate, so TLS terminates
    /// here and the hop carries ciphertext — which is the only arrangement
    /// an enclave can use, since it has no network of its own.
    ///
    /// # Errors
    ///
    /// Returns [`UpstreamError::Unavailable`] when the client cannot be built.
    pub fn via(
        base: &str,
        key: &str,
        trust: &str,
        ceiling: u32,
        vouched: Arc<dyn Registry>,
        through: Option<std::net::SocketAddr>,
    ) -> Result<Self, UpstreamError> {
        let mut client = Client::builder().pool_idle_timeout(IDLE).read_timeout(STALL);
        if let Some(address) = through {
            let host = base
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or_default();
            client = client.resolve(host, address);
        }
        Ok(Self {
            client: client.build().map_err(|_| UpstreamError::Unavailable)?,
            base: base.trim_end_matches('/').to_owned(),
            key: key.to_owned(),
            trust: trust.to_owned(),
            ceiling,
            catalogue: RwLock::new(Vec::new()),
            vouched,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Prices for one model, read once and kept.
    async fn priced(&self, model: &str) -> Result<Listing, UpstreamError> {
        if let Some(listing) = find(&self.catalogue.read().await, model) {
            return Ok(listing);
        }
        let fetched = self.catalogue().await?;
        let listing = find(&fetched, model);
        *self.catalogue.write().await = fetched;
        listing.ok_or(UpstreamError::Malformed("a model it does not sell"))
    }
}

fn find(catalogue: &[Listing], model: &str) -> Option<Listing> {
    catalogue
        .iter()
        .find(|listing| listing.model == model)
        .cloned()
}

#[async_trait]
impl Upstream for Router {
    async fn catalogue(&self) -> Result<Vec<Listing>, UpstreamError> {
        // Authenticated: unauthenticated the catalogue is only what 0G runs
        // itself, which is a fraction of what a key can reach.
        let response = self
            .client
            .get(self.url("/models"))
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(|_| UpstreamError::Unavailable)?;
        let listed: Value = refused(response)
            .await?
            .json()
            .await
            .map_err(|_| UpstreamError::Malformed("a catalogue that is not JSON"))?;
        Ok(listed
            .get("data")
            .and_then(Value::as_array)
            .map(|models| models.iter().filter_map(listing).collect())
            .unwrap_or_default())
    }

    async fn ask(
        &self,
        ask: Ask,
    ) -> Result<stream::BoxStream<'static, Result<Served, UpstreamError>>, UpstreamError> {
        let listing = self.priced(&ask.model).await?;
        if listing.rates().most(&ask.body, self.ceiling) > ask.budget {
            return Err(UpstreamError::Unaffordable);
        }
        let response = refused(
            self.client
                .post(self.url("/chat/completions"))
                .bearer_auth(&self.key)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(TRUST, &self.trust)
                .body(ask.body)
                .send()
                .await
                .map_err(|_| UpstreamError::Unavailable)?,
        )
        .await?;

        // Which call this was, so the provider can be asked afterwards what
        // it signed for it.
        let served = response
            .headers()
            .get(RESPONSE_KEY)
            .and_then(|key| key.to_str().ok())
            .map(ToOwned::to_owned);
        let vouching = Vouching {
            client: self.client.clone(),
            vouched: self.vouched.clone(),
            served,
        };

        if !streaming(&response) {
            return Ok(stream::iter(whole(response, &vouching).await?).boxed());
        }
        Ok(relaying(response, vouching))
    }
}

fn streaming(response: &Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .is_some_and(|kind| kind.starts_with("text/event-stream"))
}

/// One frame of a stream, or the end of one.
///
/// The frame carrying the bill is read rather than relayed: it states what
/// the call cost us, and the buyer is quoted a different number.
fn framed(data: &str, tokens: &mut Tokens) -> Option<Result<Served, UpstreamError>> {
    if data.trim() == DONE {
        return None;
    }
    let Ok(chunk) = serde_json::from_str::<Value>(data) else {
        return Some(Err(UpstreamError::Malformed("a frame that is not JSON")));
    };
    if let Some(counted) = chunk.get("usage").and_then(counted) {
        *tokens = counted;
    }
    chunk.get(TRACE).map_or_else(
        || Some(Ok(Served::Frame(data.as_bytes().to_vec()))),
        |trace| Some(receipt(trace, *tokens).map(Served::Done)),
    )
}

/// What it takes to ask a provider what it signed, carried alongside the
/// stream it signed for.
#[derive(Clone)]
struct Vouching {
    client: Client,
    vouched: Arc<dyn Registry>,
    served: Option<String>,
}

/// What the provider says it signed over the call.
#[derive(Deserialize)]
struct Signed {
    text: String,
    signature: String,
}

impl Vouching {
    /// Whether the answer came from the key an attested quote named.
    ///
    /// The signature is fetched after the answer, so nothing here is on the
    /// path a caller waits on, and only from the endpoint pinned beside the
    /// signer — a URL this side chose, not one the reply suggested.
    async fn attest(&self, provider: Option<Address>) -> Attested {
        let (Some(provider), Some(served)) = (provider, self.served.as_ref()) else {
            return Attested::Absent;
        };
        let Some(attestor) = self.vouched.attestor(provider).await else {
            return Attested::Absent;
        };
        let signed = self
            .client
            .get(format!(
                "{}/v1/proxy/signature/{served}",
                attestor.endpoint.trim_end_matches('/')
            ))
            .timeout(VOUCH)
            .send()
            .await
            .ok();
        let Some(signed) = signed.filter(|reply| reply.status().is_success()) else {
            return Attested::Absent;
        };
        let Ok(signed) = signed.json::<Signed>().await else {
            return Attested::Absent;
        };
        if recovers(&signed, attestor.signer) {
            Attested::Verified
        } else {
            Attested::Failed
        }
    }
}

/// Whether the signature over what the provider says it served recovers to
/// the key its quote bound to hardware.
fn recovers(signed: &Signed, attested: Address) -> bool {
    let Ok(bytes) = alloy::hex::decode(signed.signature.trim_start_matches("0x")) else {
        return false;
    };
    Signature::try_from(bytes.as_slice())
        .and_then(|signature| {
            signature.recover_address_from_prehash(&eip191_hash_message(signed.text.as_bytes()))
        })
        .is_ok_and(|recovered| recovered == attested)
}

/// Relays the upstream's frames, and asks who signed the answer once it ends.
fn relaying(
    response: Response,
    vouching: Vouching,
) -> BoxStream<'static, Result<Served, UpstreamError>> {
    struct Reading {
        frames: BoxStream<'static, Result<Event, EventStreamError<reqwest::Error>>>,
        tokens: Tokens,
        vouching: Vouching,
    }
    stream::unfold(
        Reading {
            frames: response.bytes_stream().eventsource().boxed(),
            tokens: Tokens::default(),
            vouching,
        },
        |mut reading| async move {
            // A stall arrives as a decode failure rather than a timeout, so
            // the shape of the error is no guide to what happened. Either
            // way the stream is over.
            let Ok(event) = reading.frames.next().await? else {
                return Some((Err(UpstreamError::Unavailable), reading));
            };
            match framed(&event.data, &mut reading.tokens) {
                None => None,
                // The answer is complete, so there is now something to ask
                // the provider what it signed.
                Some(Ok(Served::Done(mut receipt))) => {
                    receipt.attested = reading.vouching.attest(receipt.provider).await;
                    Some((Ok(Served::Done(receipt)), reading))
                }
                Some(served) => Some((served, reading)),
            }
        },
    )
    .boxed()
}

/// A whole answer, as the two frames a streamed one would have ended with.
async fn whole(
    response: Response,
    vouching: &Vouching,
) -> Result<Vec<Result<Served, UpstreamError>>, UpstreamError> {
    let body = response
        .bytes()
        .await
        .map_err(|_| UpstreamError::Unavailable)?;
    let mut answer: Value = serde_json::from_slice(&body)
        .map_err(|_| UpstreamError::Malformed("an answer that is not JSON"))?;
    let tokens = answer.get("usage").and_then(counted).unwrap_or_default();
    let trace = answer
        .as_object_mut()
        .and_then(|answer| answer.remove(TRACE))
        .ok_or(UpstreamError::Malformed("an answer with no bill"))?;
    let served = serde_json::to_vec(&answer)
        .map_err(|_| UpstreamError::Malformed("an answer that will not re-encode"))?;
    let mut closing = receipt(&trace, tokens)?;
    closing.attested = vouching.attest(closing.provider).await;
    Ok(vec![Ok(Served::Frame(served)), Ok(Served::Done(closing))])
}

fn receipt(trace: &Value, tokens: Tokens) -> Result<Receipt, UpstreamError> {
    let cost = trace
        .pointer("/billing/total_cost_usd_micro")
        .and_then(Value::as_str)
        .and_then(|total| total.parse().ok())
        .ok_or(UpstreamError::Malformed("a bill with no total"))?;
    Ok(Receipt {
        cost,
        // The router will check a provider's signature when asked, but it
        // does not report the result — and a result it did report would be
        // its own word for it. A verdict needs the signature itself, from
        // the provider the chain names.
        attested: Attested::Absent,
        reference: trace
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        provider: trace
            .get("provider")
            .and_then(Value::as_str)
            .and_then(|provider| provider.parse().ok()),
        tokens,
    })
}

fn counted(usage: &Value) -> Option<Tokens> {
    Some(Tokens {
        prompt: usage.get("prompt_tokens")?.as_u64()?,
        completion: usage.get("completion_tokens")?.as_u64()?,
    })
}

fn listing(model: &Value) -> Option<Listing> {
    if model.get("type")?.as_str()? != CHAT {
        return None;
    }
    let pricing = model.get("pricing_usd")?;
    Some(Listing {
        model: model.get("id")?.as_str()?.to_owned(),
        prompt: micro(pricing.get("prompt")?.as_str()?)?,
        completion: micro(pricing.get("completion")?.as_str()?)?,
        max_output: model
            .get("max_completion_tokens")?
            .as_u64()?
            .try_into()
            .ok()?,
        confidential: model.get("verifiability").and_then(Value::as_str) == Some(CONFIDENTIAL),
    })
}

/// USD per token into micro-USD per million, without a float in the way:
/// these multiply into a bill.
fn micro(price: &str) -> Option<u64> {
    let (whole, fraction) = price.split_once('.').unwrap_or((price, ""));
    let digits: u64 = format!("{whole}{fraction}").parse().ok()?;
    let places = u32::try_from(fraction.len()).ok()?;
    digits.checked_mul(10_u64.checked_pow(SCALE.checked_sub(places)?)?)
}

/// Turns a refusal into one, keeping the upstream's own words: they are
/// already the shape the agent at the far end expects.
async fn refused(response: Response) -> Result<Response, UpstreamError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    // A rate limit included: its body names the code a client decides on,
    // and its header says how long to wait rather than how long we guess.
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    Err(UpstreamError::Refused {
        status: status.as_u16(),
        body: response.bytes().await.unwrap_or_default().to_vec(),
        retry_after,
    })
}
