//! Full round trip against a running chain.
//!
//! Ignored by default because it needs a deployed pool and the Go prover. Run
//! it with the addresses `script/DeployPool.s.sol` printed:
//!
//! ```text
//! RPC_URL=… PRIVATE_KEY=… POOL=… TOKEN=… VERIFIER=… REGISTRY=… \
//!   cargo test --test integration -- --ignored --nocapture
//! ```
//!
//! Each test needs its own deployment: spending the first note relies on
//! `MerklePath::only_leaf`, which holds only while the tree is empty. `make
//! integration` deploys a fresh pool per test.

use std::{env, error::Error as StdError, path::Path, process::Command};

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::{SignerSync, local::PrivateKeySigner},
    sol,
};
use veil402_enclave::{Call, Meter, sign_lock};
use veil402_sdk::{
    Config, Field, MerklePath, Note, Owner, Secret, Send, SessionKeys, Spend, Transaction, Veil,
    evm::{PoolClient, Session, Withdrawal},
    session::{EnclaveKey, Handover, Recipient},
};

const DEPOSIT: u64 = 1_000;
const WITHDRAW: u64 = 300;

sol! {
    #[sol(rpc)]
    interface IToken {
        function approve(address spender, uint256 value) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IRegistry {
        function register(address signer, bytes32 imageHash) external;
    }

    #[sol(rpc)]
    interface IPool {
        function lock(uint256 nullifier, address enclave, address seller, uint64 deadline, bytes signature) external;
        function leafCount() external view returns (uint256);
        function currentRoot() external view returns (uint256);
        function nullifierSpent(uint256 nullifier) external view returns (bool);
    }
}

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

#[tokio::test]
#[serial_test::serial]
#[ignore = "needs a deployed pool"]
async fn shields_then_withdraws() -> Result<(), Box<dyn StdError>> {
    let signer: PrivateKeySigner = var("PRIVATE_KEY")?.parse()?;
    let sender = signer.address();
    let provider = ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect(&var("RPC_URL")?)
        .await?;

    let pool_address: Address = var("POOL")?.parse()?;
    let token_address: Address = var("TOKEN")?.parse()?;
    let chain_id = provider.get_chain_id().await?;
    let pool = PoolClient::new(
        pool_address,
        token_address,
        var("VERIFIER")?.parse()?,
        chain_id,
    )?;

    let token = IToken::new(token_address, &provider);
    let contract = IPool::new(pool_address, &provider);

    // ---- shield ---------------------------------------------------------
    token
        .approve(pool_address, U256::from(DEPOSIT))
        .send()
        .await?
        .get_receipt()
        .await?;

    let owner = Owner::random()?;
    let note = Note {
        value: DEPOSIT,
        asset: pool.asset(),
        random: Secret::random()?,
    };
    let shield = provider
        .transaction_request()
        .to(pool_address)
        .input(pool.shield(note.public_key(&owner)?, DEPOSIT, b"shielded")?.into());
    let receipt = provider.send_transaction(shield).await?.get_receipt().await?;
    assert!(receipt.status(), "shield reverted");
    println!("shield gas       {}", receipt.gas_used);
    assert_eq!(contract.leafCount().call().await?, U256::from(1));

    // ---- withdraw -------------------------------------------------------
    let recipient = Address::from([0x11; 20]);
    let veil = Veil::open(Config::new(worker()?, root()?.join("prover/veil402-gnark/artifacts/transaction-v4")))?;
    let change = Send::new(
        Owner::random()?.public()?,
        DEPOSIT - WITHDRAW,
        Secret::random()?,
        b"change".to_vec(),
    )?;
    let transaction = Transaction::new(
        vec![Spend {
            note,
            owner,
            merkle: MerklePath::only_leaf()?,
        }],
        vec![change],
    )?;

    let prepared = veil
        .withdraw(
            &pool,
            transaction,
            Withdrawal::new(recipient, WITHDRAW)?,
        )
        .await?;
    assert_eq!(prepared.to, pool_address);

    // The proof must be against a root the pool recorded.
    assert_eq!(
        contract.currentRoot().call().await?,
        U256::from_be_bytes(prepared.proof.public.root.to_bytes()),
        "proved against a root the pool does not hold"
    );

    let send = provider
        .transaction_request()
        .to(prepared.to)
        .input(prepared.calldata.into());
    let receipt = provider.send_transaction(send).await?.get_receipt().await?;
    assert!(receipt.status(), "transact reverted");
    println!("transact gas     {}", receipt.gas_used);
    println!("transact tx      {:?}", receipt.transaction_hash);

    // ---- settled --------------------------------------------------------
    assert_eq!(token.balanceOf(recipient).call().await?, U256::from(WITHDRAW));
    assert_eq!(
        token.balanceOf(pool_address).call().await?,
        U256::from(DEPOSIT - WITHDRAW)
    );
    assert_eq!(contract.leafCount().call().await?, U256::from(3));
    for nullifier in prepared.proof.public.nullifiers {
        assert!(
            contract
                .nullifierSpent(U256::from_be_bytes(nullifier.to_bytes()))
                .call()
                .await?,
            "nullifier not recorded"
        );
    }
    println!("sender           {sender}");
    Ok(())
}

/// lock -> three checkpoints -> settle. Only the newest voucher is sent, so a
/// buyer who walks away still pays for what it consumed.
#[tokio::test]
#[serial_test::serial]
#[ignore = "needs a deployed pool"]
async fn meters_a_session_and_settles() -> Result<(), Box<dyn StdError>> {
    let signer: PrivateKeySigner = var("PRIVATE_KEY")?.parse()?;
    let provider = ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect(&var("RPC_URL")?)
        .await?;

    let pool_address: Address = var("POOL")?.parse()?;
    let token_address: Address = var("TOKEN")?.parse()?;
    let chain_id = provider.get_chain_id().await?;
    let pool = PoolClient::new(
        pool_address,
        token_address,
        var("VERIFIER")?.parse()?,
        chain_id,
    )?;
    let token = IToken::new(token_address, &provider);
    let contract = IPool::new(pool_address, &provider);

    // The enclave only ever signs; it needs no gas and no chain access.
    let enclave = PrivateKeySigner::random();
    IRegistry::new(var("REGISTRY")?.parse()?, &provider)
        .register(enclave.address(), alloy::primitives::B256::repeat_byte(0xA1))
        .send()
        .await?
        .get_receipt()
        .await?;

    // ---- a note to reserve ----------------------------------------------
    token
        .approve(pool_address, U256::from(DEPOSIT))
        .send()
        .await?
        .get_receipt()
        .await?;
    // Owned by a session owner, so handing its seed over delegates this note
    // and nothing else.
    let seed = Secret::random()?;
    let keys = SessionKeys::from_seed(&seed)?;
    let note = Note {
        value: DEPOSIT,
        asset: pool.asset(),
        random: SessionKeys::from_seed(&seed)?.random,
    };
    let leaf = u32::try_from(contract.leafCount().call().await?)?;
    let shield = provider
        .transaction_request()
        .to(pool_address)
        .input(pool.shield(note.public_key(&keys.owner)?, DEPOSIT, b"session")?.into());
    provider.send_transaction(shield).await?.get_receipt().await?;

    // ---- lock ------------------------------------------------------------
    let seller = Address::from([0x5E; 20]);
    let deadline = provider
        .get_block(alloy::eips::BlockId::latest())
        .await?
        .ok_or("no latest block")?
        .header
        .timestamp
        + 600;
    let session = Session {
        nullifier: keys.owner.nullifier(leaf)?,
        seller,
        deadline,
    };
    let lock = provider.transaction_request().to(pool_address).input(
        pool.lock(&session, enclave.address(), &sign_lock(&pool, &session, &enclave)?)
            .into(),
    );
    let receipt = provider.send_transaction(lock).await?.get_receipt().await?;
    assert!(receipt.status(), "lock reverted");
    println!("lock gas         {}", receipt.gas_used);

    // ---- hand the session over, sealed to the enclave --------------------
    let key = EnclaveKey::generate();
    let announcement = enclave.sign_hash_sync(&key.announcement(&pool))?.as_bytes();
    let handover = Handover {
        seed,
        value: DEPOSIT,
        asset: pool.asset(),
        leaf,
        siblings: MerklePath::only_leaf()?.siblings,
        root: Field::from_bytes(contract.currentRoot().call().await?.to_be_bytes())?,
        change_owner: Owner::random()?.public()?,
        change_seed: Secret::random()?,
        target: 0,
        seller,
        deadline,
        pool: pool_address,
        chain_id,
        node: Address::ZERO,
        tee_signer: Address::ZERO,
    };
    let sealed = Recipient::accept(key.public(), &pool, enclave.address(), &announcement)?
        .seal(&handover.to_bytes()?)?;

    // ---- three checkpoints, nothing on chain -----------------------------
    let mut meter = Meter::open(pool, &key, &sealed, session)?;
    let mut voucher = None;
    for cost in [40_u64, 110, 150] {
        meter.record(Call {
            cost,
            attestation: alloy::primitives::keccak256(cost.to_be_bytes()),
        })?;
        voucher = Some(meter.voucher(&enclave)?);
    }
    let voucher = voucher.ok_or("no voucher")?;
    assert_eq!(voucher.amount, 300, "vouchers carry the running total");
    println!("checkpoints      {} calls, {} owed", meter.calls(), voucher.amount);

    // ---- the enclave proves it, with the buyer gone ----------------------
    let veil = Veil::open(Config::new(
        worker()?,
        root()?.join("prover/veil402-gnark/artifacts/transaction-v4"),
    ))?;
    drop(voucher);
    let prepared = meter.settle(&veil, &enclave).await?;

    let send = provider
        .transaction_request()
        .to(prepared.to)
        .input(prepared.calldata.into());
    let receipt = provider.send_transaction(send).await?.get_receipt().await?;
    assert!(receipt.status(), "settle reverted");
    println!("settle gas       {}", receipt.gas_used);
    println!("settle tx        {:?}", receipt.transaction_hash);

    assert_eq!(token.balanceOf(seller).call().await?, U256::from(300));
    assert!(contract.nullifierSpent(U256::from_be_bytes(session.nullifier.to_bytes())).call().await?);
    Ok(())
}
