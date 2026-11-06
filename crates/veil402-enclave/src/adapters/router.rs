//! The 0G Compute Router: one key, one endpoint, every model it fronts.
//!
//! It routes and bills without being able to forge a reply, but it does read
//! the request — so this adapter buys inference, not privacy. Sealing the
//! body to the provider's own enclave is a second adapter beside this one,
//! and the port is shaped so nothing above has to know which is in use.

use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::{StreamExt, future, stream};
use reqwest::{Client, Response, StatusCode};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::core::ports::{
    Ask, Attested, Listing, Receipt, Served, Tokens, Upstream, UpstreamError,
};

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
}

impl Router {
    /// # Errors
    ///
    /// Returns [`UpstreamError::Unavailable`] when the client cannot be built.
    pub fn new(base: &str, key: &str, trust: &str, ceiling: u32) -> Result<Self, UpstreamError> {
        Ok(Self {
            client: Client::builder()
                .pool_idle_timeout(IDLE)
                .read_timeout(STALL)
                .build()
                .map_err(|_| UpstreamError::Unavailable)?,
            base: base.trim_end_matches('/').to_owned(),
            key: key.to_owned(),
            trust: trust.to_owned(),
            ceiling,
            catalogue: RwLock::new(Vec::new()),
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

        if !streaming(&response) {
            return Ok(stream::iter(whole(response).await?).boxed());
        }
        Ok(response
            .bytes_stream()
            .eventsource()
            .scan(Tokens::default(), |tokens, event| {
                future::ready(match event {
                    Ok(event) => framed(&event.data, tokens),
                    // A stall arrives here as a decode failure rather than a
                    // timeout, so the shape of the error is no guide to what
                    // happened. Either way the stream is over.
                    Err(_) => Some(Err(UpstreamError::Unavailable)),
                })
            })
            .boxed())
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

/// A whole answer, as the two frames a streamed one would have ended with.
async fn whole(response: Response) -> Result<Vec<Result<Served, UpstreamError>>, UpstreamError> {
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
    Ok(vec![
        Ok(Served::Frame(served)),
        Ok(Served::Done(receipt(&trace, tokens)?)),
    ])
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
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(UpstreamError::RateLimited);
    }
    Err(UpstreamError::Refused {
        status: status.as_u16(),
        body: response.bytes().await.unwrap_or_default().to_vec(),
    })
}
