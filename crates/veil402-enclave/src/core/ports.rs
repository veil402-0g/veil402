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
use veil402_sdk::wire::Rates;

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

impl Listing {
    #[must_use]
    pub const fn rates(&self) -> Rates {
        Rates {
            prompt: self.prompt,
            completion: self.completion,
            max_output: self.max_output,
        }
    }
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

/// A provider whose signing key has been attested, and where to ask it for
/// the signature over a call.
///
/// Pinned by configuration rather than discovered: the quote that binds this
/// key to real hardware is verified out of band, and pinning the endpoint
/// with it means this side never fetches from a URL somebody else chose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attestor {
    pub endpoint: String,
    /// What the attested quote's `report_data` named.
    pub signer: Address,
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
    /// The upstream's own refusal, kept whole. Theirs is already the shape
    /// an agent expects, and rewriting it would lose the code a client
    /// decides on.
    #[error("the upstream refused with {status}")]
    Refused {
        status: u16,
        body: Vec<u8>,
        /// Seconds it asked to be left alone for.
        retry_after: Option<u64>,
    },
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
