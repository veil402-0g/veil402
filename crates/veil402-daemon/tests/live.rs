//! The index against a running chain.
//!
//! Ignored by default: needs a deployed pool and the Go prover.
//!
//! ```text
//! RPC_URL=… PRIVATE_KEY=… POOL=… TOKEN=… VERIFIER=… \
//!   cargo test -p veil402-daemon --test live -- --ignored --nocapture
//! ```

use std::{
    env, error::Error as StdError, num::NonZeroU64, path::Path, process::Command, sync::Arc,
    time::Duration,
};

use alloy::{
    network::EthereumWallet,
    primitives::{Address, B256, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use veil402_daemon::{
    core::envelope::EnvelopeService,
    adapters::{ChainSource, SqliteStore, Window},
    app::{Config, Daemon, Retry, now},
    core::{index::IndexService, wallet::Minted},
};
use veil402_gateway::adapters::http::router;
use veil402_sdk::{
    Config as Proving, Note, Owner, Secret, Send, Spend, Transaction, Veil,
    evm::{PoolClient, Withdrawal},
    session::{Keyring, RootSeed},
    wire::{Message, Offer, Prompt},
};

const DEPOSIT: u64 = 1_000;

/// What the envelope reserves, and what a settlement leaves for the next one.
const CEILING: u64 = 600;
const MODEL: &str = "stub";
const TARGET: u64 = 200;

fn var(name: &str) -> Result<String, Box<dyn StdError>> {
    env::var(name).map_err(|_| format!("{name} is not set").into())
}

fn root_dir() -> Result<&'static Path, Box<dyn StdError>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "workspace root is unavailable".into())
}

fn worker() -> Result<std::path::PathBuf, Box<dyn StdError>> {
    let repository = root_dir()?;
    let binary = repository.join("target/veil402-gnark-test");
    let status = Command::new("go")
        .args(["build", "-o"])
        .arg(&binary)
        .arg(".")
        .current_dir(repository.join("prover/veil402-gnark"))
        .status()?;
    if !status.success() {
        return Err("Go worker build failed".into());
    }
    Ok(binary)
}

/// The whole point of the index: spend a note that is not the first, which
/// needs siblings that exist nowhere on chain.
#[tokio::test]
#[ignore = "needs a deployed pool"]
async fn spends_the_second_note() -> Result<(), Box<dyn StdError>> {
    let signer: PrivateKeySigner = var("PRIVATE_KEY")?.parse()?;
    let provider = ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect(&var("RPC_URL")?)
        .await?;

    let pool_address: Address = var("POOL")?.parse()?;
    let token_address: Address = var("TOKEN")?.parse()?;
    let pool = PoolClient::new(
        pool_address,
        token_address,
        var("VERIFIER")?.parse()?,
        provider.get_chain_id().await?,
    )?;

    // ---- two notes, so the one we spend is not leaf zero ------------------
    approve(&provider, token_address, pool_address, DEPOSIT * 2).await?;
    let first = shield(&provider, &pool, pool_address, 7).await?;
    let second = shield(&provider, &pool, pool_address, 11).await?;

    // ---- index -----------------------------------------------------------
    let file = tempfile::NamedTempFile::new()?;
    let store = Arc::new(
        SqliteStore::open(&format!("sqlite://{}?mode=rwc", file.path().display())).await?,
    );
    let source = Arc::new(ChainSource::new(
        provider.clone(),
        pool_address,
        Window {
            chunk: NonZeroU64::new(1_000).ok_or("chunk")?,
            confirmations: 0,
        },
    ));
    let index = IndexService::load(source, store).await?;

    let health = index.sync().await?;
    assert_eq!(health.leaves, 2);
    println!(
        "indexed          {} leaves, current={}",
        health.leaves,
        index.is_current().await?
    );

    // ---- spend leaf 1 ------------------------------------------------------
    let path = index.witness(&[1]).await?.paths.remove(0);
    assert_eq!(path.index, 1);
    assert_ne!(
        path.siblings[0],
        veil402_sdk::Field::ZERO,
        "leaf 1's first sibling is leaf 0, not an empty subtree"
    );

    let veil = Veil::open(Proving::new(
        worker()?,
        root_dir()?.join("prover/veil402-gnark/artifacts/transaction-v4"),
    ))?;
    let recipient = Address::from([0x22; 20]);
    let transaction = Transaction::new(
        vec![Spend {
            note: second.0,
            owner: second.1,
            merkle: path,
        }],
        vec![Send::new(
            Owner::random()?.public()?,
            DEPOSIT - 300,
            Secret::random()?,
            b"change".to_vec(),
        )?],
    )?;
    let prepared = veil
        .withdraw(&pool, transaction, Withdrawal::new(recipient, 300)?)
        .await?;

    let send = provider
        .transaction_request()
        .to(prepared.to)
        .input(prepared.calldata.into());
    let receipt = provider.send_transaction(send).await?.get_receipt().await?;
    assert!(receipt.status(), "spending the second note reverted");
    println!("spent leaf 1     {} gas", receipt.gas_used);

    drop(first);
    Ok(())
}

async fn approve(
    provider: &impl Provider,
    token: Address,
    spender: Address,
    amount: u64,
) -> Result<(), Box<dyn StdError>> {
    alloy::sol! {
        #[sol(rpc)]
        interface IToken { function approve(address spender, uint256 value) external returns (bool); }
    }
    IToken::new(token, provider)
        .approve(spender, U256::from(amount))
        .send()
        .await?
        .get_receipt()
        .await?;
    Ok(())
}

async fn shield(
    provider: &impl Provider,
    pool: &PoolClient,
    address: Address,
    seed: u64,
) -> Result<(Note, Owner), Box<dyn StdError>> {
    let owner = Owner::random()?;
    let note = Note {
        value: DEPOSIT,
        asset: pool.asset(),
        random: Secret::random()?,
    };
    let call = pool.shield(note.public_key(&owner)?, DEPOSIT, &seed.to_be_bytes())?;
    let request = provider.transaction_request().to(address).input(call.into());
    provider.send_transaction(request).await?.get_receipt().await?;
    Ok((note, owner))
}

/// The milestone: an envelope settles and the next one opens from its change.
///
/// Everything before this is machinery. This is the first point where a
/// private metered payment works end to end, and the last assertion is the
/// claim itself — a session running for hours costs one split, ever.
#[tokio::test]
#[ignore = "needs a deployed pool"]
async fn an_envelope_rolls_into_the_next() -> Result<(), Box<dyn StdError>> {
    let signer: PrivateKeySigner = var("PRIVATE_KEY")?.parse()?;
    let provider = ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect(&var("RPC_URL")?)
        .await?;
    let pool_address: Address = var("POOL")?.parse()?;
    let token_address: Address = var("TOKEN")?.parse()?;
    let pool = PoolClient::new(
        pool_address,
        token_address,
        var("VERIFIER")?.parse()?,
        provider.get_chain_id().await?,
    )?;

    // ---- the enclave the registry will vouch for -------------------------
    let root = [0x0A_u8; 32];
    let seller = Address::repeat_byte(0x5E);
    let enclave = Keyring::new(RootSeed::from_bytes(root)).signer()?.address();
    let registry = IRegistry::new(var("REGISTRY")?.parse()?, &provider);
    if !registry.isLive(enclave).call().await? {
        registry
            .register(enclave, B256::repeat_byte(0xA1))
            .send()
            .await?
            .get_receipt()
            .await?;
    }

    // ---- a cold, wallet-owned note: the only split this session ever pays
    approve(&provider, token_address, pool_address, DEPOSIT).await?;
    let owner = Owner::random()?;
    let note = Note {
        value: DEPOSIT,
        asset: pool.asset(),
        random: Secret::random()?,
    };
    let commitment = note.commitment(&owner)?;
    let call = pool.shield(note.public_key(&owner)?, DEPOSIT, b"cold")?;
    let request = provider
        .transaction_request()
        .to(pool_address)
        .input(call.into());
    let receipt = provider.send_transaction(request).await?.get_receipt().await?;
    println!("shield gas       {}", receipt.gas_used);

    // ---- the gateway that hosts the enclave ------------------------------
    let housed = tempfile::tempdir()?;
    let served = serve(&pool, root, seller, housed.path()).await?;

    // ---- the daemon, from its own composition root -----------------------
    let state = tempfile::tempdir()?;
    let daemon = Arc::new(
        daemon(
            &pool,
            owner,
            &served,
            seller,
            Minted {
                commitment,
                value: DEPOSIT,
                asset: pool.asset(),
                random: Secret::from_bytes(note.random.to_bytes())?,
                seed: None,
            },
            state.path(),
        )
        .await?,
    );

    let mining = keep_mining(provider.clone());
    let syncing = tokio::spawn({
        let daemon = daemon.clone();
        async move { daemon.run().await }
    });
    let envelopes = daemon.envelopes().ok_or("no envelope service")?.clone();
    caught_up(&daemon, commitment).await?;

    // ---- open: splits the cold note, locks, hands the session over -------
    let first = envelopes.open(MODEL, seller, CEILING, 600).await?;
    assert_eq!(first.ceiling, CEILING, "the note's value is the ceiling");
    println!("opened           ceiling {}, deadline {}", first.ceiling, first.deadline);

    // ---- meter, then settle ----------------------------------------------
    let owed = metered(&envelopes, first.nullifier).await?;
    envelopes.settle(first.nullifier, false).await?;

    let token = IToken::new(token_address, &provider);
    assert_eq!(token.balanceOf(seller).call().await?, U256::from(owed));
    assert!(
        IPoolView::new(pool_address, &provider)
            .nullifierSpent(U256::from_be_bytes(first.nullifier.to_bytes()))
            .call()
            .await?,
        "the settlement did not spend the nullifier"
    );

    // ---- the claim: the next envelope needs no split ---------------------
    let second = envelopes.open(MODEL, seller, TARGET, 600).await?;
    assert_ne!(second.nullifier, first.nullifier);
    assert_eq!(
        second.ceiling, TARGET,
        "the settlement minted the next envelope's note already seeded"
    );
    println!("rolled over      into {} without a split", second.ceiling);

    syncing.abort();
    mining.abort();
    Ok(())
}


alloy::sol! {
    #[sol(rpc)]
    interface IToken { function balanceOf(address account) external view returns (uint256); }

    #[sol(rpc)]
    interface IRegistry {
        function register(address signer, bytes32 imageHash) external;
        function isLive(address signer) external view returns (bool);
    }

    #[sol(rpc)]
    interface IPoolView { function nullifierSpent(uint256 nullifier) external view returns (bool); }
}


/// A note the index has not placed has no nullifier, and a reservation needs
/// one — so the daemon has to be caught up before it can serve anything.
async fn caught_up(daemon: &Daemon, commitment: veil402_sdk::Field) -> Result<(), Box<dyn StdError>> {
    for _ in 0..80 {
        if daemon.wallet().by_commitment(commitment).await?.is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("the daemon never placed the shielded note".into())
}


/// Anvil mines only when a transaction arrives, so a confirmation depth is
/// never reached while the daemon sits waiting for one. Real chains keep
/// producing blocks; this makes the test chain behave like one.
fn keep_mining(provider: impl Provider + 'static) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let _: Result<(), _> = provider
                .raw_request::<_, ()>("anvil_mine".into(), (1_u64,))
                .await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
}


/// The daemon as it runs in production, over a throwaway database, already
/// holding the note the test shielded. Nothing recovers a note from the
/// chain yet, so a wallet is only ever told about one.
async fn daemon(
    pool: &PoolClient,
    owner: Owner,
    gateway: &str,
    seller: Address,
    note: Minted,
    state: &Path,
) -> Result<Daemon, Box<dyn StdError>> {
    let daemon = Daemon::start(Config {
        rpc: var("RPC_URL")?,
        pool: pool.address(),
        token: var("TOKEN")?.parse()?,
        verifier: var("VERIFIER")?.parse()?,
        database: format!("sqlite://{}?mode=rwc", state.join("veil.db").display()),
        signer: var("PRIVATE_KEY")?,
        gateway: gateway.to_owned(),
        seller,
        ceiling: CEILING,
        lock_for: 600,
        wallet_state: state.join("wallet.redb"),
            worker: worker()?,
            artifacts: root_dir()?.join("prover/veil402-gnark/artifacts/transaction-v4"),
        owner,
        target: TARGET,
        window: Window {
            chunk: NonZeroU64::new(1_000).ok_or("chunk")?,
            confirmations: 0,
        },
        retry: Retry {
            interval: Duration::from_secs(1),
            attempts: 8,
        },
    })
    .await?;
    daemon.wallet().remember(&[note], now()).await?;
    Ok(daemon)
}


/// The gateway, on an ephemeral port. It holds the enclave the daemon talks
/// to, which is the arrangement production runs: the buyer's daemon never
/// holds an enclave key.
async fn serve(
    pool: &PoolClient,
    root: [u8; 32],
    seller: Address,
    state: &Path,
) -> Result<String, Box<dyn StdError>> {
    // The real reader: M1 locks honestly, so the gateway's escrow check has a
    // real escrow and a real root to find.
    let escrows = Arc::new(veil402_gateway::adapters::connect(&var("RPC_URL")?, pool.address()).await?);
    let service = veil402_gateway::app::assemble(
        &veil402_gateway::app::Config {
            pool: *pool,
            root,
        worker: worker()?,
        artifacts: root_dir()?.join("prover/veil402-gnark/artifacts/transaction-v4"),
            offers: vec![Offer {
                model: "stub".to_owned(),
                seller,
                // One token unit per unit consumed, so a test counts in whole
                // numbers.
                rate: 1,
            }],
            state: state.join("sessions.redb"),
            bind: "127.0.0.1:0".parse()?,
            rpc: var("RPC_URL")?,
        },
        escrows,
    )?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let served = format!("http://{}", listener.local_addr()?);
    let app = router(service);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(served)
}

/// Three requests through the loop, so the registry, the sealing and the
/// metering are exercised the way an agent would exercise them.
async fn metered(
    envelopes: &EnvelopeService,
    _nullifier: veil402_sdk::Field,
) -> Result<u64, Box<dyn StdError>> {
    let mut owed = 0;
    for sequence in 0..3 {
        owed = envelopes
            .dispatch(
                &Prompt {
                    model: MODEL.to_owned(),
                    messages: vec![Message {
                        role: "user".to_owned(),
                        content: "x".repeat(40),
                    }],
                    stream: false,
                    reply_to: [0; 32],
                },
                &format!("call-{sequence}"),
            )
            .await?
            .metered
            .total;
    }
    assert!(owed > 0, "three calls cost something");
    Ok(owed)
}
