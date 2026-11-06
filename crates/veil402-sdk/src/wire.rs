//! The daemon-to-gateway contract.
//!
//! JSON rather than a packed encoding: nothing here is hashed or signed — the
//! delegation inside is already authenticated by HPKE — so the readable form
//! costs nothing and a failing request can be read off the wire.

use alloy::primitives::{Address, B256};
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

impl AcceptBody {
    /// # Errors
    ///
    /// Returns [`Error::Handover`] when the envelope is not hex of the right
    /// shape.
    pub fn sealed(&self) -> Result<Sealed, Error> {
        let encapsulated: [u8; 32] = bytes(&self.encapsulated)?
            .try_into()
            .map_err(|_| Error::Handover("encapsulated key is malformed"))?;
        Ok(Sealed {
            encapsulated,
            ciphertext: bytes(&self.ciphertext)?,
        })
    }
}

/// What the buyer consumed. Never what it costs: the meter prices it, or a
/// buyer could name its own bill.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct CallBody {
    pub units: u64,
}

/// What one metered call did to the envelope.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MeteredBody {
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

/// One model a seller will serve, and what it charges.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Offer {
    pub model: String,
    pub seller: Address,
    /// Token units per unit consumed.
    pub rate: u64,
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
    /// The same seller, again.
    Retry,
    /// Another seller.
    Reshop,
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
    pub rate: u64,
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
