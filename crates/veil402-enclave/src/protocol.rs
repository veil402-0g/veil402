//! What crosses the vsock boundary.
//!
//! The host and the enclave are two binaries that ship separately: once an
//! image is measured and signed, neither end can be changed without the other
//! noticing. So this is a compatibility surface, not an internal detail, and
//! it is written down in one place rather than implied by two.
//!
//! Frames are length-prefixed borsh: deterministic, and it rejects trailing
//! bytes rather than ignoring them, which is the property that matters on a
//! boundary where one side is measured and the other is not.

use alloy::primitives::Address;
use borsh::{BorshDeserialize, BorshSerialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use veil402_sdk::{
    Field,
    evm::Session,
    session::Sealed,
    wire::{AgreeBody, MeteredBody, Offer, SettlementBody, Witness},
};

/// A field element as it travels: bytes, not a `Field`.
///
/// `Field` validates that it holds a canonical BN254 element, and a derived
/// deserialiser would not. Crossing as bytes means the check happens on the
/// way in, where it belongs, rather than being skipped because the type
/// looked like it had already done it.
pub type Bytes32 = [u8; 32];

/// A reservation, flattened for the wire.
#[derive(BorshDeserialize, BorshSerialize, Clone, Copy, Debug)]
pub struct Reserved {
    pub nullifier: Bytes32,
    pub seller: [u8; 20],
    pub deadline: u64,
}

impl From<Session> for Reserved {
    fn from(session: Session) -> Self {
        Self {
            nullifier: session.nullifier.to_bytes(),
            seller: session.seller.into(),
            deadline: session.deadline,
        }
    }
}

impl TryFrom<Reserved> for Session {
    type Error = LinkError;

    fn try_from(wire: Reserved) -> Result<Self, LinkError> {
        Ok(Self {
            nullifier: Field::from_bytes(wire.nullifier).map_err(|_| LinkError::Malformed)?,
            seller: Address::from(wire.seller),
            deadline: wire.deadline,
        })
    }
}

/// An HPKE envelope, flattened for the wire.
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
pub struct Envelope {
    pub encapsulated: Bytes32,
    pub ciphertext: Vec<u8>,
}

impl From<&Sealed> for Envelope {
    fn from(sealed: &Sealed) -> Self {
        Self {
            encapsulated: sealed.encapsulated,
            ciphertext: sealed.ciphertext.clone(),
        }
    }
}

impl From<Envelope> for Sealed {
    fn from(wire: Envelope) -> Self {
        Self {
            encapsulated: wire.encapsulated,
            ciphertext: wire.ciphertext,
        }
    }
}

/// Longest frame either side will read. A sealed answer is bounded by the
/// upstream's own output cap; anything beyond this is a host that is not
/// speaking this protocol.
const MAX_FRAME: u32 = 8 * 1024 * 1024;

/// What the enclave is for: which pool, which upstream, what it charges.
///
/// Lent rather than measured, which is a real cost: a parent can tell a
/// freshly started enclave to settle against a different pool. The enclave
/// closes that by taking these once — the first set it is given is the set
/// it keeps, and a later posting that disagrees is refused rather than
/// applied. So the window is a restart, not the whole session.
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    pub pool: [u8; 20],
    pub token: [u8; 20],
    pub verifier: [u8; 20],
    pub chain_id: u64,
    /// Where the models are bought, and the key that buys them.
    pub upstream: String,
    pub upstream_key: String,
    pub trust: String,
    pub ceiling: u32,
    /// Pool units per micro-USD, and basis points over cost.
    pub scale: u64,
    pub margin_bps: u64,
}

/// What the parent lends so the enclave can reach KMS.
///
/// Credentials only, and they authorise nothing on their own: the key policy
/// decides, and no credential satisfies it without the attestation. The
/// parent can therefore hand these over without handing over the seed.
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
pub struct Lent {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub region: String,
    /// Where the parent's proxy is listening for KMS traffic.
    pub kms_port: u32,
    /// The key to agree against. An ARN, never an alias — an alias is a
    /// pointer somebody else can move.
    pub key: String,
    /// Pinned on the first posting that succeeds.
    pub settings: Settings,
}

/// One call from the host.
#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub enum Request {
    /// Credentials and settings. Sent on a timer rather than once, so an
    /// enclave that restarted is serving again within the interval without
    /// anybody noticing it went.
    ///
    /// Answered `Ready` whether it built something or found it already
    /// built: the parent is reconciling, not commanding.
    Bootstrap(Box<Lent>),
    Signer,
    Catalogue,
    Agree(Reserved),
    Accept { sealed: Envelope, session: Reserved },
    Release(Bytes32),
    Holds(Bytes32),
    Record { nullifier: Bytes32, attempt: String, sealed: Envelope },
    Resume(Kept),
    Settle { nullifier: Bytes32, dust: bool },
}

/// One frame back.
///
/// Every request is answered by exactly one of these, except `Record`, which
/// answers `Opened` and then streams `Frame`/`Bill` until `Done` or `Failed`.
#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub enum Reply {
    /// The seed loaded, and what it commits to — which is the only thing
    /// about it that may leave.
    Ready { commitment: String },
    Signer([u8; 20]),
    Catalogue(Vec<Offer>),
    Agreed(Consented),
    Accepted { root: Bytes32 },
    Released,
    Holds(bool),
    Resumed(Vec<Replayed>),
    Settled(SettlementBody),
    /// A stream is starting. The encapsulated key every frame opens under.
    Opened { head: [u8; 32] },
    /// One sealed frame, relayed without being read.
    Frame(Vec<u8>),
    /// What the call cost. Last before `Done`.
    Bill { metered: Metered, record: Vec<u8> },
    Done,
    Failed(Fault),
}

/// What the host kept so a restarted enclave can pick a session back up.
#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct Kept {
    pub sealed: Envelope,
    pub session: Reserved,
    pub log: Vec<Vec<u8>>,
}

/// A call answered before the restart.
#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct Replayed {
    pub attempt: String,
    pub total: u64,
    pub remaining: u64,
    pub attestation: [u8; 32],
}

/// A refusal, in a shape that survives the crossing.
///
/// `MeterError` carries `&'static str` and cannot travel; the variants are
/// kept apart rather than flattened to a string because the host maps them
/// onto distinct statuses, and a 409 is not a 402.
#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub enum Fault {
    Unknown,
    CeilingReached,
    Unreserved(String),
    Repeated,
    Reused,
    Refused(String),
    Upstream { status: u16, retry_after: Option<u64>, head: [u8; 32], sealed: Vec<u8> },
}

/// Consent to meter a session, flattened for the wire.
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
pub struct Consented {
    pub signer: [u8; 20],
    pub handover_key: String,
    pub announcement: String,
    pub lock: String,
}

impl From<AgreeBody> for Consented {
    fn from(body: AgreeBody) -> Self {
        Self {
            signer: body.signer.into(),
            handover_key: body.handover_key,
            announcement: body.announcement,
            lock: body.lock,
        }
    }
}

impl From<Consented> for AgreeBody {
    fn from(wire: Consented) -> Self {
        Self {
            signer: wire.signer.into(),
            handover_key: wire.handover_key,
            announcement: wire.announcement,
            lock: wire.lock,
        }
    }
}

/// What one call cost, flattened for the wire.
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
pub struct Metered {
    pub cost: u64,
    pub total: u64,
    pub remaining: u64,
    pub attestation: Bytes32,
    pub delivered: Option<Bytes32>,
    pub witness: Option<Seen>,
    pub sequence: u64,
}

/// Who served a call and how much of it there was.
#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
pub struct Seen {
    pub reference: String,
    pub prompt: u64,
    pub completion: u64,
    pub provider: Option<[u8; 20]>,
}

impl From<MeteredBody> for Metered {
    fn from(body: MeteredBody) -> Self {
        Self {
            cost: body.cost,
            total: body.total,
            remaining: body.remaining,
            attestation: body.attestation.0,
            delivered: body.delivered.map(|digest| digest.0),
            witness: body.witness.map(|seen| Seen {
                reference: seen.reference,
                prompt: seen.prompt,
                completion: seen.completion,
                provider: seen.provider.map(Into::into),
            }),
            sequence: body.sequence,
        }
    }
}

impl From<Metered> for MeteredBody {
    fn from(wire: Metered) -> Self {
        Self {
            cost: wire.cost,
            total: wire.total,
            remaining: wire.remaining,
            attestation: wire.attestation.into(),
            delivered: wire.delivered.map(Into::into),
            witness: wire.witness.map(|seen| Witness {
                reference: seen.reference,
                prompt: seen.prompt,
                completion: seen.completion,
                provider: seen.provider.map(Into::into),
            }),
            sequence: wire.sequence,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("the link is closed")]
    Closed,
    #[error("the other end sent {0} bytes, which this protocol does not carry")]
    TooLarge(u32),
    #[error("the other end sent something this protocol does not describe")]
    Malformed,
}

/// Writes one length-prefixed frame.
///
/// # Errors
///
/// Returns [`LinkError`] when the value will not encode or the link is gone.
pub async fn send<W, T>(writer: &mut W, value: &T) -> Result<(), LinkError>
where
    W: AsyncWrite + Unpin + Send,
    T: BorshSerialize + Sync,
{
    let body = borsh::to_vec(value).map_err(|_| LinkError::Malformed)?;
    let len = u32::try_from(body.len()).map_err(|_| LinkError::Malformed)?;
    if len > MAX_FRAME {
        return Err(LinkError::TooLarge(len));
    }
    writer.write_all(&len.to_le_bytes()).await.map_err(|_| LinkError::Closed)?;
    writer.write_all(&body).await.map_err(|_| LinkError::Closed)?;
    writer.flush().await.map_err(|_| LinkError::Closed)
}

/// Reads one length-prefixed frame.
///
/// # Errors
///
/// Returns [`LinkError`] when the link is gone or the frame does not decode.
pub async fn recv<R, T>(reader: &mut R) -> Result<T, LinkError>
where
    R: AsyncRead + Unpin + Send,
    T: BorshDeserialize,
{
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header).await.map_err(|_| LinkError::Closed)?;
    let len = u32::from_le_bytes(header);
    // Checked before allocating: the length is the other end's claim, and a
    // claim is not a reason to reserve eight gigabytes.
    if len > MAX_FRAME {
        return Err(LinkError::TooLarge(len));
    }
    let mut body = vec![0_u8; len as usize];
    reader.read_exact(&mut body).await.map_err(|_| LinkError::Closed)?;
    borsh::from_slice(&body).map_err(|_| LinkError::Malformed)
}
