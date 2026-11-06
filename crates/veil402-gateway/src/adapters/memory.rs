use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use veil402_sdk::{
    Field,
    wire::Offer,
};

use crate::core::{Prices, Replay, Served};

/// The prices this gateway serves at.
///
/// One list, set at boot. The portal makes it a seller's to change, and a
/// price already bound to an open envelope does not move with it.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    offers: Vec<Offer>,
}

impl Catalogue {
    #[must_use]
    pub const fn new(offers: Vec<Offer>) -> Self {
        Self { offers }
    }
}

impl Prices for Catalogue {
    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn cheapest(&self) -> Option<&Offer> {
        self.offers.iter().min_by_key(|offer| offer.rate)
    }
}

/// Calls already answered, held in memory.
///
/// Losing them on restart costs a double-bill only for a call in flight at
/// that moment, and the enclave's own total is gone then too — so durability
/// here buys nothing the enclave does not also need.
#[derive(Debug, Default)]
pub struct Replays {
    answered: Mutex<HashMap<(Field, String), Replay>>,
}

#[async_trait]
impl Served for Replays {
    async fn replayed(&self, nullifier: Field, attempt: &str) -> Option<Replay> {
        self.answered
            .lock()
            .ok()?
            .get(&(nullifier, attempt.to_owned()))
            .cloned()
    }

    async fn remember(&self, nullifier: Field, attempt: &str, replay: &Replay) {
        if let Ok(mut answered) = self.answered.lock() {
            answered.insert((nullifier, attempt.to_owned()), replay.clone());
        }
    }
}
