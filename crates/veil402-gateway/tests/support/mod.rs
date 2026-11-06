//! Fakes shared by the gateway suites.

#![allow(dead_code)]

use std::sync::Mutex;

use alloy::primitives::Address;
use async_trait::async_trait;
use std::sync::Arc;

use futures_util::StreamExt;
use veil402_gateway::core::{Escrow, Escrows, MeterError, MeteringService};
use veil402_sdk::{
    Field, Note, Secret, SessionKeys, Tree,
    evm::{PoolClient, Session},
    session::{Exchange, Handover, Keyring, Recipient, RootSeed, Sealed},
    wire::Prompt,
};

/// A pool that holds whatever the test says it holds.
pub struct Pool {
    escrow: Mutex<Option<Escrow>>,
    known: Mutex<bool>,
    /// What the chain says the time is. A deadline is judged against this.
    now: Mutex<u64>,
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
            now: Mutex::new(0),
        }
    }

    /// A pool that holds nothing, which is what a buyer who never locked
    /// looks like.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            escrow: Mutex::new(None),
            known: Mutex::new(true),
            now: Mutex::new(0),
        }
    }

    /// Moves the chain's clock, so a deadline can be made to approach.
    #[must_use]
    pub fn at(self, now: u64) -> Self {
        *self.now.lock().expect("now") = now;
        self
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
    async fn now(&self) -> Result<u64, MeterError> {
        Ok(*self.now.lock().expect("now"))
    }
}

/// An upstream that answers in pieces and charges by the question's length.
///
/// Length rather than a constant so a suite can tell one call's bill from
/// another's, which is what the restart tests actually assert on.
#[derive(Default)]
pub struct Upstream {
    pub frames: usize,
    /// Give up after the frames instead of billing, the way a provider that
    /// drops mid-answer would.
    pub breaks: bool,
    /// Refuse before a byte is served, the way one does for a request it
    /// will not take.
    pub refuses: Option<u16>,
}

#[async_trait]
impl veil402_enclave::Upstream for Upstream {
    async fn catalogue(&self) -> Result<Vec<veil402_enclave::Listing>, veil402_enclave::UpstreamError> {
        Ok(vec![veil402_enclave::Listing {
            model: "stub".to_owned(),
            prompt: 1,
            completion: 1,
            max_output: 1_024,
            confidential: true,
        }])
    }

    async fn ask(
        &self,
        ask: veil402_enclave::Ask,
    ) -> Result<
        futures_util::stream::BoxStream<
            'static,
            Result<veil402_enclave::Served, veil402_enclave::UpstreamError>,
        >,
        veil402_enclave::UpstreamError,
    > {
        if let Some(status) = self.refuses {
            return Err(veil402_enclave::UpstreamError::Refused {
                status,
                body: REFUSAL.as_bytes().to_vec(),
                retry_after: Some(42),
            });
        }
        let cost = ask.body.len() as u64;
        if cost > ask.budget {
            return Err(veil402_enclave::UpstreamError::Unaffordable);
        }
        let mut served: Vec<_> = (0..self.frames)
            .map(|frame| Ok(veil402_enclave::Served::Frame(format!("part {frame}").into_bytes())))
            .collect();
        if self.breaks {
            served.push(Err(veil402_enclave::UpstreamError::Unavailable("test".into())));
        } else {
            served.push(Ok(veil402_enclave::Served::Done(veil402_enclave::Receipt {
                cost,
                reference: "stub".to_owned(),
                ..veil402_enclave::Receipt::default()
            })));
        }
        Ok(futures_util::stream::iter(served).boxed())
    }
}

/// What a refusing upstream says. `OpenAI`'s own envelope, because that is
/// what every upstream worth relaying speaks.
pub const REFUSAL: &str = r#"{"error":{"message":"too long","type":"invalid_request_error","code":"context_length_exceeded"}}"#;

/// The seed every gateway in these suites derives its keys from.
pub const ROOT: [u8; 32] = [7; 32];
/// What the one note in these suites is worth.
pub const VALUE: u64 = 1_000;
pub const DEADLINE: u64 = 1_900_000_000;

/// The pool these suites pretend to settle against.
///
/// # Errors
///
/// Returns an error when the addresses do not form a client.
pub fn pool() -> Result<PoolClient, veil402_sdk::Error> {
    PoolClient::new(
        Address::repeat_byte(0x4A),
        Address::repeat_byte(0x7B),
        Address::repeat_byte(0x9C),
        296,
    )
}

/// A pool backing the one session these tests open. The enclave's signer is
/// derived from the root, so the escrow has to name the same address.
pub fn backing(deadline: u64) -> Result<Arc<Pool>, Box<dyn std::error::Error>> {
    let signer = Keyring::new(RootSeed::from_bytes(ROOT)).signer()?.address();
    Ok(Arc::new(Pool::backing(
        signer,
        Address::repeat_byte(0x5E),
        deadline,
    )))
}

/// Hands a real delegation over, against a note that is really in a tree.
pub async fn delegate(service: &MeteringService) -> Result<Delegated, Box<dyn std::error::Error>> {
    let client = pool()?;
    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let note = Note {
        value: VALUE,
        asset: client.asset(),
        random: SessionKeys::from_seed(&seed)?.random,
    };
    let mut tree = Tree::new();
    let leaf = tree.insert(note.commitment(&keys.owner)?)?;
    let witness = tree.witness(&[leaf])?;
    let session = Session {
        nullifier: keys.owner.nullifier(leaf)?,
        seller: Address::repeat_byte(0x5E),
        deadline: DEADLINE,
    };

    let agreed = service.agree(&session).await?;
    let handover = Handover {
        seed,
        value: VALUE,
        asset: client.asset(),
        leaf,
        siblings: witness.paths[0].siblings,
        root: witness.root,
        change_seed: Secret::random()?,
        view: [0; 32],
        seller: session.seller,
        deadline: session.deadline,
        pool: client.address(),
        chain_id: client.chain_id(),
        node: Address::ZERO,
        tee_signer: Address::ZERO,
    };
    let key: [u8; 32] = veil402_sdk::wire::bytes(&agreed.handover_key)?
        .try_into()
        .map_err(|_| "key")?;
    let sealed = Recipient::accept(
        key,
        &client,
        agreed.signer,
        &veil402_sdk::wire::bytes(&agreed.announcement)?,
    )?
    .seal(&handover.to_bytes()?)?;

    service.accept(&sealed, session).await?;
    Ok(Delegated {
        nullifier: session.nullifier,
        sealing: Recipient::accept(
            key,
            &client,
            agreed.signer,
            &veil402_sdk::wire::bytes(&agreed.announcement)?,
        )?,
    })
}

/// What a daemon keeps after handing a delegation over: which envelope, and
/// the key its requests seal to.
pub struct Delegated {
    pub nullifier: Field,
    pub sealing: Recipient,
}

/// A request of a given size, sealed the way a daemon seals one.
pub fn asking(sealing: &Recipient, size: usize) -> Result<Sealed, Box<dyn std::error::Error>> {
    Ok(asked(sealing, size)?.sealed)
}

/// A sealed request, and the channel its answer comes back over.
pub struct Asked {
    pub sealed: Sealed,
    pub exchange: Exchange,
}

/// The same request, for a caller that means to read what comes back.
///
/// # Errors
///
/// Returns an error when the request will not seal.
pub fn asked(sealing: &Recipient, size: usize) -> Result<Asked, Box<dyn std::error::Error>> {
    let exchange = Exchange::open();
    let prompt = Prompt {
        body: format!(r#"{{"model":"stub","messages":[{{"role":"user","content":"{}"}}]}}"#,
                      "x".repeat(size))
            .into_bytes(),
        reply_to: exchange.public(),
    };
    Ok(Asked {
        sealed: sealing.seal_prompt(&borsh::to_vec(&prompt)?)?,
        exchange,
    })
}

