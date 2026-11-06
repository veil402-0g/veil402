//! The daemon-to-gateway contract.
//!
//! JSON rather than a packed encoding: nothing here is hashed or signed — the
//! delegation inside is already authenticated by HPKE — so the readable form
//! costs nothing and a failing request can be read off the wire.

use alloy::primitives::{Address, B256};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::{Field, client::Error, session::Sealed};

/// Names the session a request acts on.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionBody {
    pub nullifier: String,
    pub seller: Address,
    pub deadline: u64,
}

impl SessionBody {
    #[must_use]
    pub fn new(session: &crate::evm::Session) -> Self {
        Self {
            nullifier: hex::encode(session.nullifier.to_bytes()),
            seller: session.seller,
            deadline: session.deadline,
        }
    }

    /// # Errors
    ///
    /// Returns [`Error::Field`] when the nullifier is not a field element.
    pub fn parse(&self) -> Result<crate::evm::Session, Error> {
        Ok(crate::evm::Session {
            nullifier: field(&self.nullifier)?,
            seller: self.seller,
            deadline: self.deadline,
        })
    }
}

/// What an enclave returns when it agrees to meter a session.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgreeBody {
    pub signer: Address,
    pub handover_key: String,
    pub announcement: String,
    pub lock: String,
}

/// A delegation sealed to the key `agree` named.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AcceptBody {
    pub session: SessionBody,
    pub encapsulated: String,
    pub ciphertext: String,
}

impl CallBody {
    #[must_use]
    pub fn new(sealed: &Sealed) -> Self {
        Self {
            encapsulated: encode(&sealed.encapsulated),
            ciphertext: encode(&sealed.ciphertext),
        }
    }

    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the envelope is not hex of the right
    /// shape.
    pub fn sealed(&self) -> Result<Sealed, Error> {
        sealed(&self.encapsulated, &self.ciphertext)
    }
}

fn sealed(encapsulated: &str, ciphertext: &str) -> Result<Sealed, Error> {
    let key: [u8; 32] = bytes(encapsulated)?
        .try_into()
        .map_err(|_| Error::Handover("encapsulated key is malformed"))?;
    Ok(Sealed {
        encapsulated: key,
        ciphertext: bytes(ciphertext)?,
    })
}

impl AcceptBody {
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the envelope is not hex of the right
    /// shape.
    pub fn sealed(&self) -> Result<Sealed, Error> {
        sealed(&self.encapsulated, &self.ciphertext)
    }
}

/// A request, sealed to the enclave that will serve it.
///
/// The host between them relays bytes: it learns which session is working and
/// how much that costs, never what was asked or what came back.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CallBody {
    pub encapsulated: String,
    pub ciphertext: String,
}

/// What the buyer asked for, once the enclave has opened it.
///
/// The body is the agent's own request, byte for byte. Nothing here parses
/// it into fields: a shape of our own would quietly drop whatever `OpenAI`
/// adds next, and the whole point is that an agent changes a URL and nothing
/// else.
#[derive(Clone, Debug, Deserialize, Serialize, BorshDeserialize, BorshSerialize)]
pub struct Prompt {
    pub body: Vec<u8>,
    /// Where to seal the answer.
    pub reply_to: [u8; 32],
}

/// What one metered call did to the envelope.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MeteredBody {
    /// What this call cost on its own, which is what gets filed against the
    /// agent that made it. The total cannot be differenced for it: calls
    /// overlap.
    pub cost: u64,
    pub total: u64,
    pub remaining: u64,
    pub attestation: B256,
}

/// A settlement, as much of it as the sender needs.
///
/// Not the proof: the chain verifies that, and the daemon only broadcasts.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SettlementBody {
    pub key: String,
    pub calldata: String,
}

/// One model this seller will serve, and what it charges for it.
///
/// Derived from what the enclave can buy rather than configured beside it:
/// two lists of models that can disagree is one list too many.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Offer {
    pub model: String,
    /// Pool units per million tokens.
    pub prompt: u64,
    pub completion: u64,
    /// Most one call may produce.
    pub max_output: u32,
}

impl Offer {
    #[must_use]
    pub const fn rates(&self) -> Rates {
        Rates {
            prompt: self.prompt,
            completion: self.completion,
            max_output: self.max_output,
        }
    }
}

/// Bytes per token, chosen to over-count rather than under-count a prompt
/// nothing here tokenises. An estimate is an authorisation, and a capture
/// above one is not something an upstream lets us take back — so it has to
/// be an upper bound, and code or CJK tokenise far denser than prose.
/// Prompts cost a fraction of what completions do, so the slack is cheap.
const BYTES_PER_TOKEN: u64 = 2;

/// Tokens in a million, since upstreams price per token in fractions of a
/// cent and the smallest honest unit is an integer.
const MILLION: u64 = 1_000_000;

/// What a model costs per million tokens, in whatever unit the quoter counts
/// in: the enclave buys in micro-USD and sells in pool units, and the sum is
/// the same either way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rates {
    pub prompt: u64,
    pub completion: u64,
    pub max_output: u32,
}

impl Rates {
    /// Most a call could cost at these rates.
    ///
    /// The bound is `max_tokens` and never the output we expect: a reasoning
    /// model can spend an entire budget thinking and return nothing, so what
    /// it is allowed to produce is the only figure that bounds the bill.
    #[must_use]
    pub fn most(self, body: &[u8], cap: u32) -> u64 {
        let asked = u64::from(wanted(body).unwrap_or(cap).min(self.max_output));
        let sent = body.len() as u64 / BYTES_PER_TOKEN;
        (asked.saturating_mul(self.completion) / MILLION)
            .saturating_add(sent.saturating_mul(self.prompt) / MILLION)
    }
}

/// The ceiling the request set for itself, if it set one.
#[must_use]
pub fn wanted(body: &[u8]) -> Option<u32> {
    let request: serde_json::Value = serde_json::from_slice(body).ok()?;
    request
        .get("max_tokens")
        .or_else(|| request.get("max_completion_tokens"))?
        .as_u64()?
        .try_into()
        .ok()
}

/// What the caller should do next.
///
/// The reason a reply carries more than a status code: a status says what
/// went wrong, and the only question the daemon has is what to do about it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Open an envelope; the reply carries what it would take.
    Pay,
    /// The same request, again.
    Retry,
    /// Nothing will help; the agent's request fails.
    Fail,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Status {
    Ok,
    Error,
}

/// The envelope every gateway reply carries.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Reply<T> {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

impl<T> Reply<T> {
    #[must_use]
    pub const fn ok(result: T) -> Self {
        Self {
            status: Status::Ok,
            result: Some(result),
            error: None,
            action: None,
        }
    }

    pub fn failed(error: &impl ToString, action: Action) -> Self {
        Self {
            status: Status::Error,
            result: None,
            error: Some(error.to_string()),
            action: Some(action),
        }
    }

    /// A refusal that still says what it would take: the 402 carries the
    /// challenge in `result`.
    pub fn owing(result: T, error: &impl ToString) -> Self {
        Self {
            status: Status::Error,
            result: Some(result),
            error: Some(error.to_string()),
            action: Some(Action::Pay),
        }
    }
}

/// The payment a seller requires before it will serve.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Challenge {
    pub scheme: String,
    pub pool: Address,
    pub chain_id: u64,
    pub seller: Address,
    /// Enclave measurements a buyer must pin before paying.
    pub pcrs: Vec<String>,
}

/// # Errors
///
/// Returns [`Error::Field`] when the text is not 32 bytes of hex.
pub fn field(text: &str) -> Result<Field, Error> {
    let decoded: [u8; 32] = bytes(text)?.try_into().map_err(|_| Error::Field)?;
    Field::from_bytes(decoded)
}

/// # Errors
///
/// Returns [`Error::Field`] when the text is not hex.
pub fn bytes(text: &str) -> Result<Vec<u8>, Error> {
    hex::decode(text.strip_prefix("0x").unwrap_or(text)).map_err(|_| Error::Field)
}

#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    hex::encode(bytes)
}
