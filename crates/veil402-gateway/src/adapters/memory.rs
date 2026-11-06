use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use veil402_sdk::Field;

use crate::core::{Replay, Served};

/// Which calls this process has already answered.
///
/// In memory because it is a dedupe window, not a record: what survives a
/// restart is the enclave's own sealed journal, and this is rebuilt from it.
#[derive(Default)]
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
