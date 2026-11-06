use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use veil402_sdk::{
    Field,
    evm::Session,
    session::Sealed,
    wire::{AgreeBody, MeteredBody, Offer, SettlementBody},
};

use super::MeterError;

/// What the host must check, now that the enclave has opened the delegation
/// and can say what it claims.
pub struct Accepted {
    /// The tree the note is claimed to sit in. The enclave proved the path
    /// reaches it; only a chain can say the pool ever did.
    pub root: Field,
}

/// The pool's own record of a reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Escrow {
    pub enclave: Address,
    pub seller: Address,
    pub deadline: u64,
    pub settled: bool,
}

/// What the host reads from the chain on the enclave's behalf.
///
/// The enclave has no network by design, and this is the half of the check it
/// therefore cannot make: whether the reservation it was handed is real.
#[async_trait]
pub trait Escrows: Send + Sync {
    /// The reservation against a nullifier, if the pool holds one.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the chain cannot be read.
    async fn escrow(&self, nullifier: Field) -> Result<Option<Escrow>, MeterError>;

    /// Whether the pool has ever reached this root.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the chain cannot be read.
    async fn is_known_root(&self, root: Field) -> Result<bool, MeterError>;
}

/// A call the enclave had already answered before it restarted.
pub struct Replayed {
    pub attempt: String,
    pub total: u64,
    pub remaining: u64,
    pub attestation: alloy::primitives::B256,
}

/// What one metered call cost, and the enclave's own record of it.
pub struct Metering {
    pub metered: MeteredBody,
    /// Sealed to the enclave. The host keeps it and cannot read it.
    pub record: Vec<u8>,
}

/// An answer on its way, and the bill that closes it.
pub enum Billed {
    /// One sealed frame, to be relayed without being read.
    Frame(Vec<u8>),
    /// What the call cost. Always last, because an upstream prices work
    /// only once it has done it.
    Bill(Metering),
}

/// Basis points in one.
const BPS: u128 = 10_000;

/// What this seller charges for what an upstream charged it.
///
/// Two numbers rather than one multiplier, because they are two different
/// kinds of thing. The scale is a fact about the token the pool holds; the
/// margin is a decision. Folding them together also makes any margin under
/// a doubling inexpressible, since a whole-number multiplier has no way to
/// say "a tenth more".
#[derive(Clone, Copy, Debug)]
pub struct Pricing {
    /// Pool units per micro-USD. One, for a six-decimal stablecoin.
    scale: u64,
    /// Basis points over cost. Zero sells at cost.
    margin: u64,
}

impl Pricing {
    #[must_use]
    pub const fn new(scale: u64, margin: u64) -> Self {
        Self { scale, margin }
    }

    /// What to charge for an upstream bill.
    #[must_use]
    pub fn charge(self, micro_usd: u64) -> u64 {
        let gross = u128::from(micro_usd)
            * u128::from(self.scale)
            * (BPS + u128::from(self.margin))
            / BPS;
        u64::try_from(gross).unwrap_or(u64::MAX)
    }

    /// The largest upstream bill this much headroom could still cover.
    ///
    /// The inverse of `charge`, so a call is admitted against the same
    /// arithmetic that will bill it.
    #[must_use]
    pub fn affords(self, headroom: u64) -> u64 {
        let per = u128::from(self.scale) * (BPS + u128::from(self.margin));
        if per == 0 {
            return u64::MAX;
        }
        u64::try_from(u128::from(headroom) * BPS / per).unwrap_or(u64::MAX)
    }
}

/// A call the enclave has admitted and begun.
///
/// Admission happens before this exists, so a refusal is still a status the
/// caller can act on rather than a stream that dies without one.
pub struct Opened {
    /// The encapsulated key every frame opens under. Travels once.
    pub head: [u8; 32],
    pub frames: BoxStream<'static, Result<Billed, MeterError>>,
}

/// The meter both sides trust.
///
/// A trait because it runs in this process today and behind a vsock once
/// there is hardware: that move must change one adapter and nothing else.
#[async_trait]
pub trait Meters: Send + Sync {
    /// The address the registry must vouch for, and the one an escrow has to
    /// name before this meter will serve against it.
    fn signer(&self) -> Address;

    /// What this enclave can buy, priced as this seller sells it.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the upstream cannot be read.
    async fn catalogue(&self) -> Result<Vec<Offer>, MeterError>;

    /// Agrees to meter a session, and names the key its delegation must be
    /// sealed to.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the meter will not sign.
    fn agree(&self, session: &Session) -> Result<AgreeBody, MeterError>;

    /// Takes a delegation, after checking it against the escrow it names.
    /// # Errors
    ///
    /// Returns [`MeterError`] when the delegation does not open onto the
    /// escrow it names.
    async fn accept(&self, sealed: &Sealed, session: Session) -> Result<Accepted, MeterError>;

    /// Drops a session the host could not vouch for. It never happened.
    async fn release(&self, nullifier: Field);

    /// Whether a session is live. A restart empties this, which is the one
    /// failure the daemon can repair by handing its delegation over again.
    async fn holds(&self, nullifier: Field) -> bool;

    /// Serves a sealed request, prices what it consumed, and seals the answer
    /// back to the key the request named.
    ///
    /// The host passes bytes through: only the enclave sees the request, so
    /// only the enclave can say how much of it there was. That is the same
    /// reason a provider's own token count cannot be trusted.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when the session is gone, the request does not
    /// open, or its ceiling is reached.
    async fn record(
        &self,
        nullifier: Field,
        attempt: &str,
        sealed: &Sealed,
    ) -> Result<Opened, MeterError>;

    /// Picks a session back up from what a host kept.
    ///
    /// # Errors
    ///
    /// Returns [`MeterError`] when a record was not sealed by this enclave.
    /// Returns the calls it already answered, so a retry that straddles a
    /// restart is answered rather than billed a second time.
    async fn resume(&self, kept: &Kept) -> Result<Vec<Replayed>, MeterError>;

    /// Proves the settlement. `dust` asks for the smallest one that frees the
    /// note, for an envelope nobody owes anything on.
    /// # Errors
    ///
    /// Returns [`MeterError`] when the session is gone or the proof fails.
    async fn settle(&self, nullifier: Field, dust: bool) -> Result<SettlementBody, MeterError>;
}

/// Which calls were already answered.
///
/// A retried call must not be metered twice against a total that cannot be
/// rewound, and the meter itself holds nothing durable, so this is the
/// gateway's to keep.
#[async_trait]
pub trait Served: Send + Sync {
    async fn replayed(&self, nullifier: Field, attempt: &str) -> Option<Replay>;

    async fn remember(&self, nullifier: Field, attempt: &str, replay: &Replay);
}

/// A call this gateway has already answered.
#[derive(Clone, Debug)]
pub struct Replay {
    pub metered: MeteredBody,
    /// What the request looked like, so a key used twice for two different
    /// requests can be told from one used twice for the same. Zero for a
    /// record rebuilt from a journal, which does not carry it.
    pub request: B256,
}

/// What a host keeps for an enclave that cannot keep anything.
///
/// Opaque by construction: every record is sealed by the enclave to itself,
/// so a stolen store yields nothing a chain observer does not already have.
/// The delegation is likewise the one the buyer sealed, which this side has
/// never been able to read.
#[async_trait]
pub trait Resumes: Send + Sync {
    /// Remembers the delegation a session was opened with.
    async fn opened(
        &self,
        nullifier: Field,
        sealed: &Sealed,
        session: Session,
    ) -> Result<(), MeterError>;

    /// Appends one sealed record.
    async fn append(&self, nullifier: Field, record: &[u8]) -> Result<(), MeterError>;

    /// Everything kept for a session, if anything is.
    async fn recall(&self, nullifier: Field) -> Result<Option<Kept>, MeterError>;

    /// Drops a settled session. The window a stolen store covers is sessions
    /// still open, not every session there has ever been.
    async fn forget(&self, nullifier: Field) -> Result<(), MeterError>;
}

/// What a host kept for one session.
pub struct Kept {
    pub sealed: Sealed,
    pub session: Session,
    pub log: Vec<Vec<u8>>,
}
