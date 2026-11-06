//! Fakes shared by the gateway suites.

#![allow(dead_code)]

use std::sync::Mutex;

use alloy::primitives::Address;
use async_trait::async_trait;
use veil402_gateway::core::{Escrow, Escrows, MeterError};
use veil402_sdk::Field;

/// A pool that holds whatever the test says it holds.
pub struct Pool {
    escrow: Mutex<Option<Escrow>>,
    known: Mutex<bool>,
}

impl Pool {
    /// A pool backing one honest reservation, for `enclave`.
    #[must_use]
    pub fn backing(enclave: Address, seller: Address, deadline: u64) -> Self {
        Self {
            escrow: Mutex::new(Some(Escrow {
                enclave,
                seller,
                deadline,
                settled: false,
            })),
            known: Mutex::new(true),
        }
    }

    /// A pool that holds nothing, which is what a buyer who never locked
    /// looks like.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            escrow: Mutex::new(None),
            known: Mutex::new(true),
        }
    }

    /// Reserved, but not against any tree the pool reached.
    #[must_use]
    pub fn forgetting_roots(self) -> Self {
        *self.known.lock().expect("known") = false;
        self
    }

    pub fn settled(self) -> Self {
        if let Some(escrow) = self.escrow.lock().expect("escrow").as_mut() {
            escrow.settled = true;
        }
        self
    }
}

#[async_trait]
impl Escrows for Pool {
    async fn escrow(&self, _: Field) -> Result<Option<Escrow>, MeterError> {
        Ok(*self.escrow.lock().expect("escrow"))
    }

    async fn is_known_root(&self, _: Field) -> Result<bool, MeterError> {
        Ok(*self.known.lock().expect("known"))
    }
}
