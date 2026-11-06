//! Sending pool calls under a key that is nobody's buyer.

use std::{path::Path, sync::Arc};

use alloy::{
    primitives::{Address, Bytes, U256},
    sol_types::SolCall,
};
use redb::{Database, TableDefinition};
use veil402_sdk::evm::Pool;
use walletkit::{
    Runner, Wallet,
    adapters::{
        LocalSigner, RedbStateStore, SystemClock, Transport,
        policy::{DefaultPolicyEngine, TargetAllowlist},
    },
    core::wallet::{GasEnvelope, TxIntent, TxStatus},
};

use crate::Settings;

/// Keyed by the caller's own key, so a resubmission adopts the call already
/// in flight rather than spending a second nonce sending it twice.
const HANDLES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("handles");

/// How often the sending engine is turned over: rebroadcast, confirm, bump.
const TICK: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    /// A selector this key will not sign for, whatever the target says.
    #[error("the relay does not send that call")]
    NotRelayable,
    #[error("{0}")]
    Refused(String),
}

fn refused(error: impl std::fmt::Display) -> RelayError {
    RelayError::Refused(error.to_string())
}

/// Where a submitted call has reached.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Landed {
    /// Nothing has been submitted under this key.
    Unknown,
    Pending,
    Confirmed { block: u64 },
    Failed { reason: String },
    Dropped,
}

/// The pool calls that carry their own authorisation.
///
/// Every one is answerable to a proof or the enclave's signature, never to
/// whoever sent it — which is the whole reason this key can send them. The
/// list is a selector check rather than a comment: `shield` pulls tokens
/// from the caller, so relaying it would spend the relay's own balance.
fn relayable(calldata: &[u8]) -> bool {
    let Some(selector) = calldata.get(..4) else {
        return false;
    };
    [
        Pool::lockCall::SELECTOR,
        Pool::transactCall::SELECTOR,
        Pool::settleToAddressCall::SELECTOR,
        Pool::settleToNoteCall::SELECTOR,
    ]
    .iter()
    .any(|allowed| allowed == selector)
}

pub struct Relay {
    wallet: Arc<Wallet>,
    /// Kept for the life of the relay. Dropping it stops the loop that
    /// rebroadcasts, confirms and bumps.
    _ticking: Runner,
    handles: Database,
    pool: Address,
    chain_id: u64,
}

impl Relay {
    /// Opens a relay that may only ever call the pool.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] when the endpoint, the key or either store
    /// cannot be opened.
    pub fn open(settings: &Settings, state: &Path, handles: &Path) -> Result<Self, RelayError> {
        let signer = LocalSigner::from_private_key(&settings.key).map_err(refused)?;
        let policy = DefaultPolicyEngine::new(
            vec![Box::new(TargetAllowlist::new([settings.pool]))],
            Arc::new(SystemClock),
        )
        .with_fee_caps(GasEnvelope {
            max_fee_cap: settings.fee_cap,
            max_priority_cap: settings.fee_cap,
        });
        let endpoint = settings
            .rpc
            .parse()
            .map_err(|_| RelayError::Refused("the endpoint is not a URL".into()))?;
        let transport = Transport::url(endpoint).map_err(refused)?;
        let wallet = Arc::new(
            Wallet::builder(
                Arc::new(transport),
                Arc::new(signer),
                Arc::new(policy),
            )
            .store(Arc::new(RedbStateStore::open(state).map_err(refused)?))
            .confirmations(settings.confirmations)
            .build()
            .map_err(refused)?,
        );
        Ok(Self {
            _ticking: wallet.clone().run(TICK),
            wallet,
            handles: Database::create(handles).map_err(refused)?,
            pool: settings.pool,
            chain_id: settings.chain_id,
        })
    }

    #[must_use]
    pub fn account(&self) -> Address {
        self.wallet.account()
    }

    /// Sends `calldata` to the pool, once per `key`.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] when the call is not one this key sends, or
    /// when the engine refuses it.
    pub async fn submit(&self, key: &[u8], calldata: Vec<u8>) -> Result<(), RelayError> {
        if !relayable(&calldata) {
            return Err(RelayError::NotRelayable);
        }
        if self.handle(key)?.is_some() {
            return Ok(());
        }
        let intent = TxIntent::call(
            self.chain_id,
            self.wallet.account(),
            self.pool,
            U256::ZERO,
            Bytes::from(calldata),
        );
        let sent = self.wallet.send(&intent).await.map_err(refused)?;
        let id = serde_json::to_vec(&sent.id).map_err(refused)?;
        let write = self.handles.begin_write().map_err(refused)?;
        {
            let mut table = write.open_table(HANDLES).map_err(refused)?;
            table.insert(key, id.as_slice()).map_err(refused)?;
        }
        write.commit().map_err(refused)
    }

    /// Where the call submitted under `key` has reached.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] when the store cannot be read.
    pub async fn landed(&self, key: &[u8]) -> Result<Landed, RelayError> {
        let Some(tracked) = self.handle(key)? else {
            return Ok(Landed::Unknown);
        };
        let id = serde_json::from_slice(&tracked).map_err(refused)?;
        Ok(match self.wallet.status(id).await {
            Ok(Some(TxStatus::Confirmed { block })) => Landed::Confirmed { block },
            Ok(Some(TxStatus::Failed { reason })) => Landed::Failed { reason },
            Ok(Some(TxStatus::Replaced | TxStatus::Dropped)) => Landed::Dropped,
            _ => Landed::Pending,
        })
    }

    fn handle(&self, key: &[u8]) -> Result<Option<Vec<u8>>, RelayError> {
        let read = self.handles.begin_read().map_err(refused)?;
        let table = match read.open_table(HANDLES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(refused(error)),
        };
        Ok(table
            .get(key)
            .map_err(refused)?
            .map(|found| found.value().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::relayable;
    use alloy::sol_types::SolCall;
    use veil402_sdk::evm::Pool;

    /// The guard is the whole of what stops this key being spent by whoever
    /// can reach the socket, so it is worth pinning against the bindings
    /// rather than against four literals that would survive an ABI change.
    #[test]
    fn sends_only_what_carries_its_own_authorisation() {
        for selector in [
            Pool::lockCall::SELECTOR,
            Pool::transactCall::SELECTOR,
            Pool::settleToAddressCall::SELECTOR,
            Pool::settleToNoteCall::SELECTOR,
        ] {
            let mut calldata = selector.to_vec();
            calldata.extend_from_slice(&[0; 96]);
            assert!(relayable(&calldata), "{selector:?} should relay");
        }

        // `shield` pulls tokens from whoever calls it, so relaying it would
        // spend the relay's own balance rather than the buyer's.
        let mut shield = Pool::shieldCall::SELECTOR.to_vec();
        shield.extend_from_slice(&[0; 96]);
        assert!(!relayable(&shield));

        assert!(!relayable(&[]));
        assert!(!relayable(&[0xde, 0xad, 0xbe]));
        assert!(!relayable(&[0xde, 0xad, 0xbe, 0xef]));
    }
}
