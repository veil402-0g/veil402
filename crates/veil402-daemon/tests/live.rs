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
};

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use veil402_daemon::{
    adapters::{ChainSource, SqliteStore, Window},
    core::index::IndexService,
};
use veil402_sdk::{
    Config, Note, Owner, Secret, Send, Spend, Transaction, Veil,
    evm::{PoolClient, Withdrawal},
};

const DEPOSIT: u64 = 1_000;

fn var(name: &str) -> Result<String, Box<dyn StdError>> {
    env::var(name).map_err(|_| format!("{name} is not set").into())
}

fn root() -> Result<&'static Path, Box<dyn StdError>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "workspace root is unavailable".into())
}

fn worker() -> Result<std::path::PathBuf, Box<dyn StdError>> {
    let repository = root()?;
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

    let veil = Veil::open(Config::new(
        worker()?,
        root()?.join("prover/veil402-gnark/artifacts/transaction-v4"),
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
