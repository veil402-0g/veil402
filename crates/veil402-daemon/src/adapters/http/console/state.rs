//! What a console handler is given, and the settings it serves at bootstrap.

use std::sync::Arc;

use alloy::primitives::Address;
use serde::Serialize;

use crate::core::{
    agent::AgentService, envelope::EnvelopeService, index::IndexService, wallet::WalletService,
};

/// Pool units are the token's own smallest unit, so one is a micro-USD and
/// the console divides by this to show money.
const DECIMALS: u8 = 6;

/// Addresses and constants the console would otherwise hardcode.
///
/// Served rather than compiled in: a console built against one deployment
/// and pointed at another should say so, not quietly show the wrong chain.
#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    pub chain_id: u64,
    pub pool: Address,
    pub token: Address,
    pub verifier: Address,
    /// Who this daemon buys from.
    pub seller: Address,
    pub decimals: u8,
    /// What one envelope is worth, and how long it may hold its note.
    pub session: u64,
    pub window: u64,
    /// What the console may offer. Each is a route that exists, so a button
    /// is drawn from this rather than from what the build happens to know.
    pub features: Features,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Features {
    pub deposit: bool,
    pub disclose: bool,
    pub pause: bool,
}

impl Settings {
    #[must_use]
    pub const fn new(
        chain_id: u64,
        pool: Address,
        token: Address,
        verifier: Address,
        seller: Address,
        session: u64,
        window: u64,
    ) -> Self {
        Self {
            chain_id,
            pool,
            token,
            verifier,
            seller,
            decimals: DECIMALS,
            session,
            window,
            features: Features {
                // Shielding pulls tokens from the sender and needs the
                // transaction engine the running daemon already holds, so
                // it stays a command until that is hoisted.
                deposit: false,
                disclose: false,
                pause: true,
            },
        }
    }
}

/// Everything the operator surface reads.
#[derive(Clone)]
pub struct Console {
    pub agents: Arc<AgentService>,
    pub envelopes: Arc<EnvelopeService>,
    pub index: Arc<IndexService>,
    pub wallet: Arc<WalletService>,
    pub settings: Arc<Settings>,
}
