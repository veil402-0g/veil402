//! Opening an envelope against a reserved note.
//!
//! The enclave here is real enough to matter: it derives its keys from a root
//! seed and vouches for them the way the daemon checks, so a delegation that
//! would not seal in production does not seal here either. Proving is not —
//! the split and settle paths need a worker and a chain, and M1 is where they
//! are exercised.

mod common;

use std::sync::{Arc, Mutex};

use alloy::{
    primitives::Address,
    signers::{SignerSync, local::PrivateKeySigner},
};
use async_trait::async_trait;
use veil402_daemon::{
    adapters::SqliteStore,
    core::{
        envelope::{
            Agreement, Answered, Broadcaster, ChainError, Enclave, EnclaveError, EnvelopeService,
            EnvelopeStore, Landed, ProveError, Prover, Settlement, Terms,
        },
        index::IndexService,
        wallet::{Minted, NoteStore, WalletService},
    },
};
use veil402_sdk::{
    Field, Owner, Prepared, Secret, SessionKeys, Transaction,
    evm::{PoolClient, Session, Voucher},
    session::{Keyring, RootSeed, Sealed},
    wire::Prompt,
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const CEILING: u64 = 1_000;
const WINDOW: u64 = 600;

fn pool() -> Result<PoolClient, veil402_sdk::Error> {
    PoolClient::new(
        Address::repeat_byte(0x4A),
        Address::repeat_byte(0x7B),
        Address::repeat_byte(0x9C),
        296,
    )
}

fn seller() -> Address {
    Address::repeat_byte(0x5E)
}

/// An enclave that derives real keys and signs real announcements, so the
/// daemon's `Recipient::accept` has something genuine to refuse or allow.
struct Vouching {
    pool: PoolClient,
    keyring: Keyring,
    signer: PrivateKeySigner,
    held: Mutex<Vec<Field>>,
}

impl Vouching {
    fn new(pool: PoolClient) -> Result<Self, Box<dyn std::error::Error>> {
        let keyring = Keyring::new(RootSeed::from_bytes([9; 32]));
        let signer = keyring.signer()?;
        Ok(Self {
            pool,
            keyring,
            signer,
            held: Mutex::new(Vec::new()),
        })
    }

    fn accepted(&self) -> Vec<Field> {
        self.held.lock().expect("held").clone()
    }
}

#[async_trait]
impl Enclave for Vouching {
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

    async fn chat(&self, _: Field, _: &Prompt, _: &str) -> Result<Answered, EnclaveError> {
        Err(EnclaveError::Unknown)
    }

    async fn settle(&self, _: Field, _: bool) -> Result<Settlement, EnclaveError> {
        Err(EnclaveError::Unknown)
    }
}

/// Records what was submitted, and answers with whatever the test wants the
/// chain to have decided.
struct Sending {
    verdict: Landed,
    sent: Mutex<Vec<Field>>,
}

impl Sending {
    fn new(verdict: Landed) -> Self {
        Self {
            verdict,
            sent: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Broadcaster for Sending {
    async fn submit(&self, key: Field, _: Vec<u8>) -> Result<(), ChainError> {
        self.sent.lock().expect("sent").push(key);
        Ok(())
    }

    async fn landed(&self, _: Field) -> Result<Option<Landed>, ChainError> {
        Ok(Some(self.verdict.clone()))
    }

    async fn tick(&self) -> Result<(), ChainError> {
        Ok(())
    }

    async fn timestamp(&self) -> Result<u64, ChainError> {
        Ok(1_000)
    }
}

/// Never called: a seeded note needs no split, and settling needs a worker.
struct Unused;

#[async_trait]
impl Prover for Unused {
    async fn transfer(&self, _: Transaction) -> Result<Prepared, ProveError> {
        Err(ProveError::Failed)
    }

    async fn settle(&self, _: Transaction, _: Voucher) -> Result<Prepared, ProveError> {
        Err(ProveError::Failed)
    }
}

struct Fixture {
    store: Arc<SqliteStore>,
    service: EnvelopeService,
    enclave: Arc<Vouching>,
    chain: Arc<Sending>,
    nullifier: Field,
}

/// A daemon holding one session note, placed at leaf 0 and ready to lock.
async fn fixture(verdict: Landed) -> Result<Fixture, Box<dyn std::error::Error>> {
    let client = pool()?;
    let store = Arc::new(SqliteStore::open("sqlite::memory:").await?);

    // The leaf the fake pool emits. Its value is not the note's real
    // commitment, which costs nothing here: the enclave checks that against
    // the root, and the enclave under test is the daemon's orchestration.
    let commitment = Field::from(1_u64);
    let index = Arc::new(IndexService::load(Arc::new(common::Chain::new(1)), store.clone()).await?);
    index.sync().await?;

    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let owner = Owner::random()?;
    let wallet = Arc::new(WalletService::new(store.clone(), store.clone(), owner));
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

    let index = Arc::new(
        IndexService::load(Arc::new(common::Chain::new(0)), store.clone()).await?,
    );
    let enclave = Arc::new(Vouching::new(client)?);
    let chain = Arc::new(Sending::new(verdict));
    let service = EnvelopeService::new(
        wallet,
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
    );
    Ok(Fixture {
        store,
        service,
        enclave,
        chain,
        nullifier: keys.owner.nullifier(0)?,
    })
}

/// The change seed has to be on disk before the lock leaves, or a crash
/// between them reserves a note whose change nothing can ever identify.
#[tokio::test]
async fn an_envelope_records_its_change_seed_before_it_locks() -> Fallible {
    let fixture = fixture(Landed::Confirmed { block: 2 }).await?;

    let envelope = fixture
        .service
        .open(seller(), CEILING, WINDOW)
        .await?;

    assert_eq!(envelope.ceiling, CEILING, "the note's value is the ceiling");
    assert_eq!(envelope.nullifier, fixture.nullifier);
    assert_eq!(envelope.deadline, 1_000 + WINDOW, "deadline from the chain's clock");
    assert!(
        fixture.store.change_seed(envelope.nullifier).await?.is_some(),
        "the change seed was never written"
    );
    assert_eq!(
        fixture.enclave.accepted(),
        vec![envelope.nullifier],
        "the enclave was handed a delegation that opened"
    );
    assert_eq!(*fixture.chain.sent.lock().expect("sent"), vec![envelope.nullifier]);
    Ok(())
}

/// A lock the chain refused will never hold the note, so the next session can
/// have it now rather than waiting out the reservation.
#[tokio::test]
async fn a_refused_lock_frees_the_note_again() -> Fallible {
    let fixture = fixture(Landed::Failed("AlreadyLocked".into())).await?;

    assert!(fixture.service.open(seller(), CEILING, WINDOW).await.is_err());
    assert_eq!(
        fixture.store.spendable(pool()?.asset(), 0).await?.len(),
        1,
        "the note is still held"
    );
    Ok(())
}

/// The pool refuses a reservation it would have to hold for longer than an
/// hour, and finding that out on chain costs the gas.
#[tokio::test]
async fn a_window_past_the_pool_limit_never_reaches_the_chain() -> Fallible {
    let fixture = fixture(Landed::Confirmed { block: 2 }).await?;

    assert!(fixture.service.open(seller(), CEILING, 3_601).await.is_err());
    assert!(fixture.chain.sent.lock().expect("sent").is_empty());
    Ok(())
}
