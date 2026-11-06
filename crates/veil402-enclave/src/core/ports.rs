//! What the enclave buys inference from.
//!
//! A port rather than a client because the way we reach a provider is going
//! to change twice: a sealed envelope that keeps the router from reading the
//! request, and later a broker that settles on chain. Both answer the same
//! question — give me this completion, and tell me what it cost — so neither
//! should reach past this file.

use alloy::primitives::Address;
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use thiserror::Error;

/// Tokens in a million, since upstreams price per token in fractions of a
/// cent and integer micro-USD is the smallest honest unit.
const MILLION: u64 = 1_000_000;

/// Bytes per token, chosen to over-count rather than under-count a prompt
/// nothing here tokenises. An estimate is an authorisation, and a capture
/// above one is not something an upstream lets us take back — so it has to
/// be an upper bound, and code or CJK tokenise far denser than prose.
/// Prompts cost a fraction of what completions do, so the slack is cheap.
const BYTES_PER_TOKEN: u64 = 2;

/// One model an upstream will serve, and what it charges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listing {
    pub model: String,
    /// Micro-USD per million tokens.
    pub prompt: u64,
    pub completion: u64,
    /// Most one call may produce.
    pub max_output: u32,
    /// The model runs inside an enclave, rather than proxying to a vendor
    /// that does. Only these can carry the privacy claim.
    pub confidential: bool,
}

/// One inference, as the enclave asks for it.
pub struct Ask {
    pub model: String,
    /// The buyer's request, exactly as it arrived.
    pub body: Vec<u8>,
    /// Most this call may cost, in micro-USD.
    pub budget: u64,
}

/// A frame as it came off the wire, then the receipt that closes the call.
#[derive(Clone, Debug)]
pub enum Served {
    /// One upstream chunk, verbatim.
    Frame(Vec<u8>),
    /// What it cost and what vouches for it. Always last.
    Done(Receipt),
}

/// What an upstream says a call cost, and what backs the claim.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Receipt {
    /// Micro-USD, in the upstream's own accounting.
    pub cost: u64,
    pub attested: Attested,
    /// Enough to verify this call later, by someone who is not us.
    pub reference: String,
    pub provider: Option<Address>,
    pub tokens: Tokens,
}

/// What the upstream reported using. Kept beside our own count rather than
/// instead of it: a figure nobody can check is at least one we can compare.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tokens {
    pub prompt: u64,
    pub completion: u64,
}

/// Whether anything vouches for the answer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Attested {
    /// A signature was checked against attested hardware.
    Verified,
    /// One was offered and did not hold up.
    Failed,
    /// None was available. A router that relays for a provider cannot sign
    /// for it, so this is the honest answer until we fetch the signature
    /// from the provider itself.
    #[default]
    Absent,
}

#[derive(Clone, Debug, Error)]
pub enum UpstreamError {
    #[error("the upstream is unreachable")]
    Unavailable,
    #[error("the upstream is rate limiting")]
    RateLimited,
    /// The upstream's own refusal, kept whole. Theirs is already the shape an
    /// agent expects, so translating it would only lose detail.
    #[error("the upstream refused with {status}")]
    Refused { status: u16, body: Vec<u8> },
    /// The call could cost more than the envelope has left. Refused before
    /// anything is sent, because an upstream prices a call only once it has
    /// finished serving it.
    #[error("the call could cost more than is left")]
    Unaffordable,
    #[error("the upstream answered with {0}")]
    Malformed(&'static str),
}

/// Where completions come from.
#[async_trait]
pub trait Upstream: Send + Sync {
    /// What this account can buy, and for how much.
    ///
    /// # Errors
    ///
    /// Returns [`UpstreamError`] when the catalogue cannot be read.
    async fn catalogue(&self) -> Result<Vec<Listing>, UpstreamError>;

    /// Opens one call. A failure here happens before anything is owed.
    ///
    /// # Errors
    ///
    /// Returns [`UpstreamError`] when the upstream refuses, or the call
    /// could cost more than `ask.budget`.
    async fn ask(
        &self,
        ask: Ask,
    ) -> Result<BoxStream<'static, Result<Served, UpstreamError>>, UpstreamError>;
}

/// Most a call could cost, in micro-USD.
///
/// The bound is `max_tokens` and never the output we expect: a reasoning
/// model can spend an entire budget thinking and return nothing, so what it
/// is allowed to produce is the only figure that bounds the bill.
#[must_use]
pub fn estimate(listing: &Listing, body: &[u8], cap: u32) -> u64 {
    let asked = u64::from(wanted(body).unwrap_or(cap).min(listing.max_output));
    let sent = body.len() as u64 / BYTES_PER_TOKEN;
    let completion = asked.saturating_mul(listing.completion) / MILLION;
    let prompt = sent.saturating_mul(listing.prompt) / MILLION;
    completion.saturating_add(prompt)
}

/// The ceiling the request set for itself, if it set one.
fn wanted(body: &[u8]) -> Option<u32> {
    let request: serde_json::Value = serde_json::from_slice(body).ok()?;
    request
        .get("max_tokens")
        .or_else(|| request.get("max_completion_tokens"))?
        .as_u64()?
        .try_into()
        .ok()
}
