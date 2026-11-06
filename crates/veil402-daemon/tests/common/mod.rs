//! Fakes the suites share: a pool that can misbehave in each of the ways
//! the daemon has to tell apart, a store that mirrors what the real one does
//! with nodes, and a daemon wired over both with one note to spend.

#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use async_trait::async_trait;
pub use veil402_daemon::core::envelope::Landed;
use veil402_daemon::core::{
    StoreError,
    envelope::{Broadcaster, ChainError as SendError},
    index::{
        Batch, ChainError, Commitment, CommitmentSource, Funding, IndexStore, Locked, Nullified,
    },
};
use futures_util::StreamExt;
use veil402_sdk::{Field, StoredNode, Tree};

pub type Fallible = Result<(), Box<dyn std::error::Error>>;

#[must_use]
pub fn field(value: u64) -> Field {
    Field::from(value)
}

/// How the endpoint behaves: present, away entirely, or dropping three passes
/// out of four.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum Relay {
    #[default]
    Up,
    Down,
    Flaky,
}

/// A pool holding `held` commitments, one per block. `withheld` is a leaf the
/// source refuses to report while the root still counts it, which is what a
/// missed event looks like.
pub struct Chain {
    held: Vec<Field>,
    spent: Vec<Field>,
    withheld: Option<usize>,
    corrupted: Option<usize>,
    relay: Relay,
    calls: AtomicU32,
}

impl Chain {
    #[must_use]
    pub fn new(count: u64) -> Self {
        Self {
            held: (1..=count).map(field).collect(),
            spent: Vec::new(),
            withheld: None,
            corrupted: None,
            relay: Relay::Up,
            calls: AtomicU32::new(0),
        }
    }

    /// Skips a leaf entirely, which is what a dropped log looks like.
    #[must_use]
    pub fn withholding(count: u64, leaf: usize) -> Self {
        Self {
            withheld: Some(leaf),
            ..Self::new(count)
        }
    }

    /// Reports every leaf but one with the wrong value, so only the root
    /// notices.
    #[must_use]
    pub fn corrupting(count: u64, leaf: usize) -> Self {
        Self {
            corrupted: Some(leaf),
            ..Self::new(count)
        }
    }

    /// A pool that has also recorded nullifiers.
    #[must_use]
    pub fn spending(count: u64, spent: &[u64]) -> Self {
        Self {
            spent: spent.iter().copied().map(field).collect(),
            ..Self::new(count)
        }
    }

    #[must_use]
    pub fn down(count: u64) -> Self {
        Self {
            relay: Relay::Down,
            ..Self::new(count)
        }
    }

    #[must_use]
    pub fn flaky(count: u64) -> Self {
        Self {
            relay: Relay::Flaky,
            ..Self::new(count)
        }
    }

    fn refuses(&self) -> bool {
        match self.relay {
            Relay::Up => false,
            Relay::Down => true,
            Relay::Flaky => self.calls.fetch_add(1, Ordering::Relaxed) % 4 != 3,
        }
    }

    /// Every root the pool has passed through, which is what `knownRoot`
    /// remembers.
    fn roots(&self) -> Result<Vec<Field>, ChainError> {
        let mut tree = Tree::new();
        self.held
            .iter()
            .map(|commitment| {
                tree.insert(*commitment)
                    .map_err(|_| ChainError::InvalidData)?;
                tree.root().map_err(|_| ChainError::InvalidData)
            })
            .collect()
    }

    fn height(&self) -> Result<u64, ChainError> {
        u64::try_from(self.held.len()).map_err(|_| ChainError::InvalidData)
    }
}

#[async_trait]
impl CommitmentSource for Chain {
    async fn since(&self, block: u64) -> Result<Batch, ChainError> {
        if self.refuses() {
            return Err(ChainError::Unavailable);
        }
        let mut commitments = Vec::new();
        for (index, commitment) in self.held.iter().enumerate() {
            let leaf_index = u32::try_from(index).map_err(|_| ChainError::InvalidData)?;
            let at = u64::from(leaf_index) + 1;
            if at > block && self.withheld != Some(index) {
                commitments.push(Commitment {
                    leaf_index,
                    commitment: if self.corrupted == Some(index) {
                        field(u64::MAX)
                    } else {
                        *commitment
                    },
                    note: index.to_be_bytes().to_vec(),
                    block: at,
                });
            }
        }
        let nullifiers = self
            .spent
            .iter()
            .filter(|_| block == 0)
            .map(|nullifier| Nullified {
                nullifier: *nullifier,
                block: 1,
            })
            .collect();
        Ok(Batch {
            commitments,
            nullifiers,
            locks: Vec::new(),
            settlements: Vec::new(),
            through: self.height()?,
        })
    }

    async fn is_known_root(&self, root: Field) -> Result<bool, ChainError> {
        Ok(self.roots()?.contains(&root))
    }

    async fn root(&self) -> Result<Field, ChainError> {
        self.roots()?.pop().ok_or(ChainError::InvalidData)
    }
}

/// Mirrors the real store: nodes are upserted by position and come back in
/// order, and a refused write must leave the service's tree untouched.
#[derive(Default)]
pub struct Memory {
    commitments: Mutex<Vec<Commitment>>,
    nodes: Mutex<BTreeMap<(u8, u32), Field>>,
    cursor: Mutex<u64>,
    spent: Mutex<HashSet<Field>>,
    refusing: AtomicBool,
}

impl Memory {
    pub fn refuse(&self) {
        self.refusing.store(true, Ordering::Relaxed);
    }

    fn held<T>(lock: &Mutex<T>) -> Result<MutexGuard<'_, T>, StoreError> {
        lock.lock().map_err(|_| StoreError::Unavailable)
    }
}

#[async_trait]
impl IndexStore for Memory {
    async fn cursor(&self) -> Result<u64, StoreError> {
        Ok(*Self::held(&self.cursor)?)
    }

    async fn nodes(&self) -> Result<Vec<StoredNode>, StoreError> {
        Ok(Self::held(&self.nodes)?
            .iter()
            .map(|((level, index), node)| StoredNode {
                level: *level,
                index: *index,
                node: *node,
            })
            .collect())
    }

    async fn append(&self, batch: &Batch, nodes: &[StoredNode]) -> Result<(), StoreError> {
        if self.refusing.load(Ordering::Relaxed) {
            return Err(StoreError::Unavailable);
        }
        Self::held(&self.commitments)?.extend(batch.commitments.iter().cloned());
        Self::held(&self.spent)?.extend(batch.nullifiers.iter().map(|spent| spent.nullifier));
        let mut held = Self::held(&self.nodes)?;
        for node in nodes {
            held.insert((node.level, node.index), node.node);
        }
        *Self::held(&self.cursor)? = batch.through;
        Ok(())
    }

    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError> {
        Ok(Self::held(&self.commitments)?
            .iter()
            .find(|held| held.commitment == commitment)
            .map(|held| held.leaf_index))
    }

    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError> {
        Ok(Self::held(&self.spent)?.contains(&nullifier))
    }

    async fn notes(&self, from: u32, limit: u32) -> Result<Vec<Commitment>, StoreError> {
        Ok(Self::held(&self.commitments)?
            .iter()
            .filter(|held| held.leaf_index >= from)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn escrows(&self, _: u32) -> Result<Vec<Locked>, StoreError> {
        Ok(Vec::new())
    }
}

/// A key with nothing left to shield.
pub struct Unfunded;

#[async_trait]
impl Funding for Unfunded {
    async fn depositable(&self) -> Result<u64, ChainError> {
        Ok(0)
    }
}

#[async_trait]
impl veil402_daemon::core::treasury::Funder for Unfunded {
    fn account(&self) -> Address {
        Address::ZERO
    }

    async fn allow(
        &self,
        _: Address,
        _: u64,
    ) -> Result<bool, veil402_daemon::core::envelope::ChainError> {
        Ok(false)
    }

    async fn send(
        &self,
        _: Address,
        _: Vec<u8>,
    ) -> Result<(), veil402_daemon::core::envelope::ChainError> {
        Err(veil402_daemon::core::envelope::ChainError::Unavailable)
    }
}

/// A broadcaster that sends nothing.
///
/// The sync loop's subject is the index and the notes; what it writes is
/// exercised where a chain is real.
#[derive(Default)]
pub struct Silent;

#[async_trait]
impl Broadcaster for Silent {
    async fn submit(&self, _: Field, _: Vec<u8>) -> Result<(), SendError> {
        Ok(())
    }

    async fn landed(&self, _: Field) -> Result<Option<Landed>, SendError> {
        Ok(None)
    }


    async fn timestamp(&self) -> Result<u64, SendError> {
        Ok(0)
    }
}

// --- a daemon wired over fakes, shared by the suites that need one --------

use alloy::{
    primitives::Address,
    signers::{SignerSync, local::PrivateKeySigner},
};
use veil402_daemon::core::treasury::TreasuryService;
use veil402_daemon::{
    adapters::{Api, Console, Settings, SqliteStore, router},
    core::{
        agent::AgentService,
        envelope::{
            Agreement, Enclave, EnclaveError, EnvelopeService, Metered, Piece, ProveError, Prover,
            Settlement, Terms,
        },
        index::IndexService,
        wallet::{Minted, WalletService},
    },
};
use veil402_sdk::{
    Owner, Prepared, Secret, SessionKeys, Transaction,
    evm::{PoolClient, Session, Voucher},
    session::{Keyring, RootSeed, Sealed},
    wire::Offer,
};

pub const CEILING: u64 = 1_000;
pub const WINDOW: u64 = 600;
pub const MODEL: &str = "stub";

/// A second model the seller sells, so a mandate that names one can be told
/// from a mandate that names none.
pub const OTHER: &str = "stub-mini";
/// What the fake enclave says every call cost.
pub const COST: u64 = 7;
/// Output assumed when a request names no limit of its own.
pub const CAP: u32 = 1_024;

/// # Errors
pub fn pool() -> Result<PoolClient, veil402_sdk::Error> {
    PoolClient::new(
        Address::repeat_byte(0x4A),
        Address::repeat_byte(0x7B),
        Address::repeat_byte(0x9C),
        296,
    )
}

#[must_use]
pub fn seller() -> Address {
    Address::repeat_byte(0x5E)
}

/// An agent's request, as the bytes it arrives as.
#[must_use]
pub fn asking() -> Vec<u8> {
    format!(r#"{{"model":"{MODEL}","messages":[]}}"#).into_bytes()
}

/// An enclave that derives real keys and signs real announcements, so the
/// daemon's `Recipient::accept` has something genuine to refuse or allow.
pub struct Vouching {
    pool: PoolClient,
    keyring: Keyring,
    signer: PrivateKeySigner,
    held: Mutex<Vec<Field>>,
    frames: usize,
    refusal: Mutex<Option<EnclaveError>>,
}

impl Vouching {
    /// # Errors
    pub fn new(pool: PoolClient, frames: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let keyring = Keyring::new(RootSeed::from_bytes([9; 32]));
        let signer = keyring.signer()?;
        Ok(Self {
            pool,
            keyring,
            signer,
            held: Mutex::new(Vec::new()),
            frames,
            refusal: Mutex::new(None),
        })
    }

    /// Makes every call from here on refuse the way an upstream would.
    pub fn refusing(&self, refusal: EnclaveError) {
        *self.refusal.lock().expect("refusal") = Some(refusal);
    }

    #[must_use]
    pub fn accepted(&self) -> Vec<Field> {
        self.held.lock().expect("held").clone()
    }
}

#[async_trait]
impl Enclave for Vouching {
    async fn catalogue(&self) -> Result<Vec<Offer>, EnclaveError> {
        Ok([MODEL, OTHER]
            .into_iter()
            .map(|model| Offer {
                model: model.to_owned(),
                prompt: 500_000,
                completion: 2_000_000,
                max_output: 4_096,
            })
            .collect())
    }

    async fn agree(&self, session: &Session) -> Result<Agreement, EnclaveError> {
        let key = self.keyring.handover(session.nullifier);
        let announcement = self
            .signer
            .sign_hash_sync(&key.announcement(&self.pool))
            .map_err(|error| EnclaveError::Refused(error.to_string()))?;
        Ok(Agreement {
            signer: self.signer.address(),
            handover_key: key.public(),
            announcement: announcement.as_bytes().to_vec(),
            lock: vec![0; 65],
        })
    }

    async fn accept(&self, sealed: Sealed, session: Session) -> Result<(), EnclaveError> {
        // Opening it is the check: a delegation sealed to another envelope's
        // key, or altered in flight, does not come back.
        self.keyring
            .handover(session.nullifier)
            .open(&sealed)
            .map_err(|error| EnclaveError::Refused(error.to_string()))?;
        self.held.lock().expect("held").push(session.nullifier);
        Ok(())
    }

    async fn chat(
        &self,
        nullifier: Field,
        _: &[u8],
        _: &str,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<Piece, EnclaveError>>, EnclaveError>
    {
        if let Some(refusal) = self.refusal.lock().expect("refusal").clone() {
            return Err(refusal);
        }
        let mut pieces: Vec<Result<Piece, EnclaveError>> = (0..self.frames)
            .map(|frame| {
                Ok(Piece::Body(
                    format!(
                        r#"{{"id":"stub","object":"chat.completion.chunk","choices":[{{"index":0,"delta":{{"content":"{frame}"}}}}]}}"#
                    )
                    .into_bytes(),
                ))
            })
            .collect();
        pieces.push(Ok(Piece::Billed(Metered {
            cost: COST,
            total: COST,
            remaining: CEILING - COST,
            attestation: alloy::primitives::B256::repeat_byte(0xA7),
            delivered: None,
            witness: None,
            sequence: 0,
            // The envelope it was asked to meter: a canned one would hide
            // every mistake in attributing a call to its escrow.
            nullifier,
        })));
        Ok(futures_util::stream::iter(pieces).boxed())
    }

    async fn settle(&self, _: Field, _: bool) -> Result<Settlement, EnclaveError> {
        Err(EnclaveError::Unknown)
    }
}

/// Records what was submitted, and answers with whatever the test wants the
/// chain to have decided.
pub struct Sending {
    verdict: Landed,
    pub sent: Mutex<Vec<Field>>,
}

impl Sending {
    #[must_use]
    pub fn new(verdict: Landed) -> Self {
        Self {
            verdict,
            sent: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Broadcaster for Sending {
    async fn submit(&self, key: Field, _: Vec<u8>) -> Result<(), SendError> {
        self.sent.lock().expect("sent").push(key);
        Ok(())
    }

    async fn landed(&self, _: Field) -> Result<Option<Landed>, SendError> {
        Ok(Some(self.verdict.clone()))
    }

    async fn timestamp(&self) -> Result<u64, SendError> {
        Ok(1_000)
    }
}

/// Never called: a seeded note needs no split, and settling needs a worker.
pub struct Unused;

#[async_trait]
impl Prover for Unused {
    async fn transfer(&self, _: Transaction) -> Result<Prepared, ProveError> {
        Err(ProveError::Failed)
    }

    async fn settle(&self, _: Transaction, _: Voucher) -> Result<Prepared, ProveError> {
        Err(ProveError::Failed)
    }
}

pub struct Fixture {
    pub store: Arc<SqliteStore>,
    pub wallet: Arc<WalletService>,
    pub service: Arc<EnvelopeService>,
    pub agents: Arc<AgentService>,
    pub index: Arc<IndexService>,
    pub enclave: Arc<Vouching>,
    pub chain: Arc<Sending>,
    pub nullifier: Field,
}

impl Fixture {
    #[must_use]
    pub fn console(&self) -> Console {
        Console {
            agents: self.agents.clone(),
            envelopes: self.service.clone(),
            index: self.index.clone(),
            wallet: self.wallet.clone(),
            payment: Arc::new(veil402_daemon::core::payment::PaymentService::new(
                self.wallet.clone(),
                self.index.clone(),
                Arc::new(Unused),
                self.chain.clone(),
                self.store.clone(),
                pool().expect("pool"),
            )),
            receipts: self.store.clone(),
            funding: Arc::new(Unfunded),
            treasury: Arc::new(TreasuryService::new(
                self.wallet.clone(),
                Arc::new(Unfunded),
                pool().expect("pool"),
                Owner {
                    spend: Secret::random().expect("spend"),
                    view: Secret::random().expect("view"),
                },
            )),
            settings: Arc::new(Settings::new(1, seller(), seller(), seller(), seller(), CEILING, WINDOW)),
        }
    }

    /// Both surfaces, as the daemon serves them.
    pub fn served(&self) -> axum::Router {
        router(self.api(), self.console(), &[])
    }

    #[must_use]
    pub fn api(&self) -> Api {
        Api {
            agents: self.agents.clone(),
            envelopes: self.service.clone(),
            index: self.index.clone(),
            cap: CAP,
            started: 0,
        }
    }
}

/// A daemon holding one session note, placed at leaf 0 and ready to lock.
///
/// # Errors
pub async fn fixture(
    verdict: Landed,
    frames: usize,
) -> Result<Fixture, Box<dyn std::error::Error>> {
    let client = pool()?;
    let store = Arc::new(SqliteStore::open("sqlite::memory:").await?);

    // The leaf the fake pool emits. Its value is not the note's real
    // commitment, which costs nothing here: the enclave checks that against
    // the root, and what is under test is the daemon's orchestration.
    let commitment = field(1);
    let index = Arc::new(IndexService::load(Arc::new(Chain::new(1)), store.clone()).await?);
    index.sync().await?;

    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let wallet = Arc::new(WalletService::new(
        store.clone(),
        store.clone(),
        Owner::random()?,
    ));
    wallet
        .remember(
            &[Minted {
                commitment,
                value: CEILING,
                asset: client.asset(),
                random: Secret::from_bytes(keys.random.to_bytes())?,
                seed: Some(Secret::from_bytes(seed.to_bytes())?),
            }],
            0,
        )
        .await?;
    wallet.reconcile(0).await?;

    let index = Arc::new(IndexService::load(Arc::new(Chain::new(0)), store.clone()).await?);
    let enclave = Arc::new(Vouching::new(client, frames)?);
    let chain = Arc::new(Sending::new(verdict));
    let service = Arc::new(EnvelopeService::new(
        wallet.clone(),
        index,
        enclave.clone(),
        chain.clone(),
        Arc::new(Unused),
        store.clone(),
        Terms {
            pool: client,
            seller: seller(),
            session: CEILING,
            window: WINDOW,
        },
    ));
    Ok(Fixture {
        agents: Arc::new(AgentService::new(store.clone())),
        index: Arc::new(IndexService::load(Arc::new(Chain::new(0)), store.clone()).await?),
        store,
        wallet,
        service,
        enclave,
        chain,
        nullifier: keys.owner.nullifier(0)?,
    })
}

/// A daemon whose chain accepts everything, which is what every test of the
/// agent API is about.
///
/// # Errors
pub async fn serving(frames: usize) -> Result<Fixture, Box<dyn std::error::Error>> {
    fixture(Landed::Confirmed { block: 2 }, frames).await
}
