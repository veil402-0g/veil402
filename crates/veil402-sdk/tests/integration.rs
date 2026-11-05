//! Full round trip against a running chain.
//!
//! Ignored by default because it needs a deployed pool and the Go prover. Run
//! it with the addresses `script/DeployPool.s.sol` printed:
//!
//! ```text
//! RPC_URL=… PRIVATE_KEY=… POOL=… TOKEN=… VERIFIER=… \
//!   cargo test --test integration -- --ignored --nocapture
//! ```

use std::{env, error::Error as StdError, path::Path, process::Command};

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
};
use veil402_sdk::{
    Config, MerklePath, Note, Owner, Secret, Send, Spend, Transaction, Veil,
    evm::{PoolClient, Withdrawal},
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
    interface IPool {
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
