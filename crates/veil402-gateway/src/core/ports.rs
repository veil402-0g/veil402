use async_trait::async_trait;
use veil402_sdk::{
    Field,
    evm::Session,
    session::Sealed,
    wire::{AgreeBody, MeteredBody, Offer, SettlementBody},
};

use super::MeterError;

/// The meter both sides trust.
///
/// A trait because it runs in this process today and behind a vsock once
/// there is hardware: that move must change one adapter and nothing else.
#[async_trait]
pub trait Meters: Send + Sync {
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
    async fn accept(&self, sealed: &Sealed, session: Session) -> Result<(), MeterError>;

    /// Whether a session is live. A restart empties this, which is the one
    /// failure the daemon can repair by handing its delegation over again.
    async fn holds(&self, nullifier: Field) -> bool;

    /// Meters a call that cost `price`.
    /// # Errors
    ///
    /// Returns [`MeterError`] when the session is gone or its ceiling is
    /// reached.
    async fn record(
        &self,
        nullifier: Field,
        price: u64,
        units: u64,
    ) -> Result<MeteredBody, MeterError>;

    /// Proves the settlement. `dust` asks for the smallest one that frees the
    /// note, for an envelope nobody owes anything on.
    /// # Errors
    ///
    /// Returns [`MeterError`] when the session is gone or the proof fails.
    async fn settle(&self, nullifier: Field, dust: bool) -> Result<SettlementBody, MeterError>;
}

/// What this gateway will serve, and for how much.
pub trait Prices: Send + Sync {
    fn offers(&self) -> &[Offer];

    /// The seller a buyer is routed to. Cheapest wins; a buyer that wants
    /// another one opens its envelope against that seller directly.
    fn cheapest(&self) -> Option<&Offer>;
}

/// Which calls were already answered.
///
/// A retried call must not be metered twice against a total that cannot be
/// rewound, and the meter itself holds nothing durable, so this is the
/// gateway's to keep.
#[async_trait]
pub trait Served: Send + Sync {
    async fn replayed(&self, nullifier: Field, attempt: &str) -> Option<MeteredBody>;

    async fn remember(&self, nullifier: Field, attempt: &str, metered: &MeteredBody);
}
