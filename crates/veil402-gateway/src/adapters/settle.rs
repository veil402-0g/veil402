//! Sending settlements the buyer never asked for.

use std::{path::Path, sync::Arc, time::Duration};

use alloy::{
    primitives::{Address, Bytes, U256},
    sol,
    sol_types::SolCall,
};
use async_trait::async_trait;
use veil402_sdk::Field;
use walletkit::{
    Runner, Wallet,
    adapters::{
        LocalSigner, RedbStateStore, SystemClock, Transport,
        policy::{DefaultPolicyEngine, TargetAllowlist},
    },
    core::wallet::{GasEnvelope, SimOutcome, TxIntent},
};

use crate::core::{MeterError, Settlement, Settles};

/// What a sender needs beyond the endpoint it sends through.
#[derive(Clone, Copy, Debug)]
pub struct Sending {
    /// The only two addresses this key may ever reach.
    pub pool: Address,
    pub multicall: Address,
    pub chain_id: u64,
    pub confirmations: u64,
    /// Most it will pay per unit of gas.
    pub fee_cap: u128,
}

sol! {
    struct Call3 {
        address target;
        bool allowFailure;
        bytes callData;
    }

    function aggregate3(Call3[] calls) external payable returns (bytes[] results);
}

/// How often the sending engine is turned over: rebroadcast, confirm, bump.
/// Independent of the sweep, because a transaction already in flight needs
/// attention whether or not there is a new one to send.
const TICK: Duration = Duration::from_secs(5);

fn refused(error: impl std::fmt::Display) -> MeterError {
    MeterError::Refused(error.to_string())
}

/// Sends settlements through `walletkit`, which owns nonces, fee bumps and
/// the reorg-aware lifecycle this would otherwise have to keep itself.
///
/// The key here pays gas and nothing else. A settlement is authorised by the
/// enclave's signature and the proof the pool verifies, and the pool never
/// looks at who sent it — so this cannot move a note, only pay for one to be
/// moved.
pub struct Settler {
    wallet: Arc<Wallet>,
    /// Kept alive for the life of the sender. Dropping it stops the loop
    /// that rebroadcasts, confirms and bumps — none of which is tied to
    /// anything being sent, and all of which still has to happen on a pass
    /// that finds nothing to do.
    _ticking: Runner,
    pool: Address,
    multicall: Address,
    chain_id: u64,
}

/// Opens a sender that may only ever reach the pool, or the batcher that
/// reaches it on our behalf.
///
/// # Errors
///
/// Returns [`MeterError`] when the endpoint, the key or the engine's own
/// store cannot be opened.
pub async fn settler(
    rpc: &str,
    key: &str,
    state: &Path,
    sending: Sending,
) -> Result<Settler, MeterError> {
    let Sending {
        pool,
        multicall,
        chain_id,
        confirmations,
        fee_cap,
    } = sending;
    let signer = LocalSigner::from_private_key(key).map_err(refused)?;
    let policy = DefaultPolicyEngine::new(
        vec![Box::new(TargetAllowlist::new([pool, multicall]))],
        Arc::new(SystemClock),
    )
    .with_fee_caps(GasEnvelope {
        max_fee_cap: fee_cap,
        max_priority_cap: fee_cap,
    });
    let store = RedbStateStore::open(state).map_err(refused)?;
    let endpoint = rpc
        .parse()
        .map_err(|_| MeterError::Refused("the endpoint is not a URL".into()))?;
    let transport = Transport::url(endpoint).map_err(refused)?;
    let wallet = Arc::new(
        Wallet::builder(Arc::new(transport), Arc::new(signer), Arc::new(policy))
            .store(Arc::new(store))
            .confirmations(confirmations)
            .build()
            .map_err(refused)?,
    );
    Ok(Settler {
        _ticking: wallet.clone().run(TICK),
        wallet,
        pool,
        multicall,
        chain_id,
    })
}

#[async_trait]
impl Settles for Settler {
    async fn send(&self, settlements: &[Settlement]) -> Result<Vec<Field>, MeterError> {
        let would = self.landing(settlements).await;
        let Some((to, calldata)) = batched(self.pool, self.multicall, &would) else {
            return Ok(Vec::new());
        };
        let intent = TxIntent::call(self.chain_id, self.wallet.account(), to, U256::ZERO, calldata);
        self.wallet.send(&intent).await.map_err(refused)?;
        Ok(would
            .iter()
            .map(|settlement| settlement.nullifier)
            .collect())
    }
}

impl Settler {
    /// The settlements that would land if sent now.
    ///
    /// A batch asks the pool to tolerate a call that fails, which is what
    /// keeps one stale settlement from taking the rest down — but it also
    /// means a failure inside one leaves no trace in the transaction. So
    /// each is simulated first and a reverting one is dropped here, where
    /// it is still the caller's to carry and will come round again.
    ///
    /// A lone settlement needs none of this: the engine estimates gas
    /// before it signs, and a transaction that would revert never goes.
    async fn landing<'a>(&self, settlements: &'a [Settlement]) -> Vec<&'a Settlement> {
        if settlements.len() < 2 {
            return settlements.iter().collect();
        }
        let mut landing = Vec::with_capacity(settlements.len());
        for settlement in settlements {
            let intent = TxIntent::call(
                self.chain_id,
                self.wallet.account(),
                self.pool,
                U256::ZERO,
                Bytes::from(settlement.calldata.clone()),
            );
            match self.wallet.dry_run(&intent).await {
                Ok(preview) => {
                    if let SimOutcome::Revert(reason) = preview.outcome {
                        tracing::warn!(?reason, "a settlement would not land, and was held back");
                    } else {
                        landing.push(settlement);
                    }
                }
                // The simulation itself failed, which says nothing about
                // the settlement. Send it and let the chain decide.
                Err(error) => {
                    tracing::debug!(%error, "a settlement could not be simulated");
                    landing.push(settlement);
                }
            }
        }
        landing
    }
}

/// One settlement goes straight to the pool; several go together.
///
/// Deadlines are spread out, so most passes carry one and the hop through a
/// batcher would buy nothing. `allowFailure` is what matters when they do
/// travel together: the buyer may have settled one of them itself a moment
/// ago, and a nullifier already spent must not take the rest down with it.
#[must_use]
pub fn batched(
    pool: Address,
    multicall: Address,
    settlements: &[&Settlement],
) -> Option<(Address, Bytes)> {
    match settlements {
        [] => None,
        [one] => Some((pool, Bytes::from(one.calldata.clone()))),
        many => Some((
            multicall,
            Bytes::from(
                aggregate3Call {
                    calls: many
                        .iter()
                        .map(|settlement| Call3 {
                            target: pool,
                            allowFailure: true,
                            callData: Bytes::from(settlement.calldata.clone()),
                        })
                        .collect(),
                }
                .abi_encode(),
            ),
        )),
    }
}
