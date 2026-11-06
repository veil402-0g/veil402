//! Settlement, seen from outside both processes.
//!
//! The gateway and the daemon each run their own composition root on a real
//! socket, with their real loops: the index, the rollover, the seller's
//! sweep. This file is an agent and nothing else — it posts chat completions
//! and then asks the chain and the wallet what happened. Nothing here
//! settles anything, and that is the point.
//!
//! ```text
//! RPC_URL=… PRIVATE_KEY=… POOL=… TOKEN=… VERIFIER=… POOL_BLOCK=… \
//! VEIL_0G_BASE=… VEIL_0G_KEY=… VEIL_ROOT_SEED=… VEIL_GATEWAY_SIGNER=… \
//!   cargo test -p veil402-daemon --test settlement -- --ignored --nocapture
//! ```

use std::{
    env,
    error::Error as StdError,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use alloy::{
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
};
use veil402_daemon::{
    adapters::{SqliteStore, Window},
    app::{Config, Daemon, Retry, deposit, now},
    core::{envelope::EnvelopeStore, wallet::NoteStore},
};
use veil402_gateway::core::Escrows;
use veil402_sdk::{Field, Owner, Secret, evm::PoolClient, wire::Offer};

type Fallible = Result<(), Box<dyn StdError>>;

/// A daemon running its own loops, which the test only ever stops.
type Running = tokio::task::JoinHandle<Result<(), veil402_daemon::app::Halted>>;

/// What a call is allowed to produce. The daemon reserves against this, so
/// the test has to size a note with the same number.
const CAP: u32 = 256;

/// Long enough that the seller's sweep — which closes anything within two
/// minutes of its deadline — does not fire the moment a session opens.
const WINDOW: u64 = 180;

/// Blocks per log query.
///
/// Wide on purpose. A cold index walks from the pool's first block to the
/// head one query at a time, so the chunk decides how long that takes — and
/// the gap grows with the chain. At a thousand it outran the patience below
/// and the test blamed the daemon for the endpoint's round trips.
const CHUNK: u64 = 20_000;

/// Turns the services' own logging on, once, so a failure here is readable.
fn watching() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

fn var(name: &str) -> Result<String, Box<dyn StdError>> {
    env::var(name).map_err(|_| format!("{name} is not set").into())
}

fn root_dir() -> Result<&'static Path, Box<dyn StdError>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or_else(|| "no repository root".into())
}

fn artifacts() -> Result<PathBuf, Box<dyn StdError>> {
    Ok(root_dir()?.join("prover/veil402-gnark/artifacts/transaction-v4"))
}

fn worker() -> Result<PathBuf, Box<dyn StdError>> {
    let binary = root_dir()?.join("target/veil402-gnark-test");
    let built = std::process::Command::new("go")
        .args(["build", "-o"])
        .arg(&binary)
        .arg(".")
        .current_dir(root_dir()?.join("prover/veil402-gnark"))
        .status()?;
    if !built.success() {
        return Err("the Go worker would not build".into());
    }
    Ok(binary)
}

fn pool() -> Result<PoolClient, Box<dyn StdError>> {
    Ok(PoolClient::new(
        var("POOL")?.parse()?,
        var("TOKEN")?.parse()?,
        var("VERIFIER")?.parse()?,
        296,
    )?)
}

/// The request every call in this file makes. Sent byte for byte, because
/// the daemon prices the bytes it is given.
fn asking() -> String {
    format!(
        r#"{{"model":"{}","messages":[{{"role":"user","content":"Reply with one word: ok"}}],"max_tokens":{CAP},"stream":true}}"#,
        model()
    )
}

fn model() -> String {
    env::var("VEIL_MODEL").unwrap_or_else(|_| "glm-5.3".to_owned())
}

/// Starts a seller on `bind`, with its own sweep running.
fn seller(bind: &str, state: &Path) -> Result<(), Box<dyn StdError>> {
    let config = veil402_gateway::app::Config {
        serving_rpc: String::new(),
        serving: alloy::primitives::Address::ZERO,
        fee_cap: 20_000_000_000_000,
        pool: pool()?,
        root: alloy::hex::decode(var("VEIL_ROOT_SEED")?)?
            .try_into()
            .map_err(|_| "VEIL_ROOT_SEED is not 32 bytes")?,
        rpc: var("RPC_URL")?,
        worker: worker()?,
        artifacts: artifacts()?,
        seller: var("VEIL_SELLER")?.parse()?,
        state: state.join("sessions.redb"),
        bind: bind.parse()?,
        upstream: var("VEIL_0G_BASE")?,
        upstream_key: var("VEIL_0G_KEY")?,
        trust: "private".to_owned(),
        scale: 1,
        margin_bps: 0,
        ceiling: CAP,
        signer: var("VEIL_GATEWAY_SIGNER")?,
        multicall: "0xcA11bde05977b3631167028862bE2a173976CA11".parse()?,
        wallet_state: state.join("gateway-wallet.redb"),
        confirmations: 0,
    };
    tokio::spawn(async move {
        let _ = veil402_gateway::app::serve(config).await;
    });
    Ok(())
}

fn buying(
    listen: &str,
    gateway: &str,
    database: &str,
    state: &Path,
    session: u64,
) -> Result<Config, Box<dyn StdError>> {
    Ok(Config {
        fee_cap: 20_000_000_000_000,
        listen: listen.parse()?,
        cap: CAP,
        console_origins: Vec::new(),
        rpc: var("RPC_URL")?,
        logs: env::var("LOGS_URL").unwrap_or(var("RPC_URL")?),
        pool: var("POOL")?.parse()?,
        token: var("TOKEN")?.parse()?,
        verifier: var("VERIFIER")?.parse()?,
        database: database.to_owned(),
        signer: var("PRIVATE_KEY")?,
        gateway: gateway.to_owned(),
        seller: var("VEIL_SELLER")?.parse()?,
        session,
        lock_for: WINDOW,
        wallet_state: state.join("wallet.redb"),
        relay: None,
        worker: worker()?,
        artifacts: artifacts()?,
        // Fixed, not random. These keys are the balance: a second handle on
        // the same wallet has to derive the same notes, or it recomputes a
        // leaf the tree never held and proves against a root nobody knows.
        owner: Owner {
            spend: Secret::from_bytes([1; 32])?,
            view: Secret::from_bytes([2; 32])?,
        },
        window: Window {
            // Where the pool was deployed. An index builds a tree, so it has
            // to see leaf zero: starting anywhere later is a gap, and a gap
            // is a divergence.
            from: var("POOL_BLOCK")?.parse()?,
            chunk: NonZeroU64::new(CHUNK).ok_or("chunk")?,
            confirmations: 0,
        },
        retry: Retry {
            interval: Duration::from_secs(2),
            attempts: 8,
        },
    })
}

/// Waits until a URL answers, so a test does not race a server's bind.
async fn up(url: &str) -> Fallible {
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if client.get(url).send().await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(format!("{url} never answered").into())
}

/// What the seller sells, so a note can be sized against the same arithmetic
/// the daemon will reserve with.
async fn offered(gateway: &str) -> Result<Offer, Box<dyn StdError>> {
    let reply: veil402_sdk::wire::Reply<Vec<Offer>> =
        reqwest::get(format!("{gateway}/v1/catalogue"))
            .await?
            .json()
            .await?;
    reply
        .result
        .unwrap_or_default()
        .into_iter()
        .find(|offer| offer.model == model())
        .ok_or_else(|| format!("{} is not sold", model()).into())
}

/// One call, as an agent makes it.
async fn ask(daemon: &str, key: &str) -> Result<String, Box<dyn StdError>> {
    let answer = reqwest::Client::new()
        .post(format!("{daemon}/v1/chat/completions"))
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(asking())
        .send()
        .await?
        .text()
        .await?;
    if answer.contains("\"error\"") {
        return Err(format!("the call was refused: {answer}").into());
    }
    Ok(answer)
}

/// The vUSD a seller holds. What a settlement moves, and the only number
/// both sides agree on.
async fn held(provider: &impl Provider, seller: Address) -> Result<U256, Box<dyn StdError>> {
    alloy::sol! {
        #[sol(rpc)]
        interface IToken { function balanceOf(address account) external view returns (uint256); }
    }
    Ok(IToken::new(var("TOKEN")?.parse::<Address>()?, provider)
        .balanceOf(seller)
        .call()
        .await?)
}

/// The ledger a running daemon issues keys from.
fn ledger(
    daemon: &Daemon,
) -> Result<&Arc<veil402_daemon::core::agent::AgentService>, Box<dyn StdError>> {
    daemon
        .agents()
        .ok_or_else(|| "the daemon serves no agents".into())
}

/// Starts a daemon's own loops: the index, the rollover, the agent API.
fn started(daemon: &Arc<Daemon>) -> Running {
    let daemon = daemon.clone();
    tokio::spawn(async move { daemon.run().await })
}

/// Waits for the pool to record a session as closed.
async fn closed(escrows: &impl Escrows, nullifier: Field) -> Fallible {
    for _ in 0..WINDOW {
        if escrows
            .escrow(nullifier)
            .await?
            .is_some_and(|escrow| escrow.settled)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("nobody closed the session".into())
}

/// A daemon that stopped is the answer to whatever we were waiting for.
///
/// Without this a halted index reads as a timeout, and the test blames the
/// wrong thing for a minute before saying so.
fn alive(running: &Running) -> Fallible {
    if running.is_finished() {
        return Err("the daemon stopped while the test was waiting on it".into());
    }
    Ok(())
}

/// Waits for the index to place a note the chain has already taken.
///
/// Read-only on purpose: asking the wallet to reserve one would claim it,
/// and what opens the envelope has to be the daemon's own choice.
async fn placed(daemon: &Daemon, running: &Running, commitment: Field) -> Fallible {
    for _ in 0..240 {
        alive(running)?;
        if daemon.wallet().by_commitment(commitment).await?.is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(format!(
        "the daemon never placed its note; the index reached {} leaves",
        daemon.index().leaves().await
    )
    .into())
}

/// Waits for a daemon that was handed nothing to find what it owns.
///
/// Read-only, like `placed`: reserving a note claims it, and the envelope
/// this is waiting to see opened needs that note still free.
async fn spendable(store: &SqliteStore, running: &Running, asset: Field) -> Fallible {
    for _ in 0..240 {
        alive(running)?;
        if !store.spendable(asset, now()).await?.is_empty() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("the daemon never found what the settlement left it".into())
}

/// An envelope runs out, and the buyer settles it without being asked. The
/// agent sees two answers and nothing else.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a deployed pool, a prover and an upstream"]
async fn the_buyer_settles_what_it_spent() -> Fallible {
    watching();
    let state = tempfile::tempdir()?;
    let gateway = "http://127.0.0.1:18401";
    seller("127.0.0.1:18401", state.path())?;
    up(&format!("{gateway}/v1/catalogue")).await?;

    // One call's worth, so the second call cannot fit it.
    let estimate = offered(gateway)
        .await?
        .rates()
        .most(asking().as_bytes(), CAP);
    let database = format!(
        "sqlite://{}?mode=rwc",
        state.path().join("veil.db").display()
    );
    let provider = ProviderBuilder::new().connect(&var("RPC_URL")?).await?;
    let config = buying(
        "127.0.0.1:18402",
        gateway,
        &database,
        state.path(),
        estimate + 1,
    )?;
    // More handles on the same settings. A Config is consumed by whatever
    // it starts, and this test starts a daemon twice with a top-up between.
    let topping = buying(
        "127.0.0.1:18402",
        gateway,
        &database,
        state.path(),
        estimate + 1,
    )?;
    let topped = buying(
        "127.0.0.1:18402",
        gateway,
        &database,
        state.path(),
        estimate + 1,
    )?;
    let note = deposit(&config, estimate + 1).await?;

    let asset = pool()?.asset();
    let paid_to: Address = var("VEIL_SELLER")?.parse()?;
    let paid = held(&provider, paid_to).await?;

    let daemon = Arc::new(Daemon::start(config).await?);
    let running = started(&daemon);
    placed(&daemon, &running, note).await?;
    let key = ledger(&daemon)?.issue("test", None, None, now()).await?.key;

    ask("http://127.0.0.1:18402", &key).await?;
    let store = SqliteStore::open(&database).await?;
    let first = store.newest().await?.ok_or("no envelope was opened")?;

    // Somewhere to roll to. A settlement turns headroom into a note of the
    // same size, so an envelope that ran out is never served by its own
    // change — there has to be something else to open. Topped up with the
    // daemon down, because nonces are coordinated through the engine's
    // store and two senders on one key would lose a transaction.
    running.abort();
    let _ = running.await;
    drop(daemon);
    let spare = deposit(&topping, estimate * 4).await?;

    let daemon = Arc::new(Daemon::start(topped).await?);
    let running = started(&daemon);
    placed(&daemon, &running, spare).await?;

    // The second call cannot fit, so the daemon settles the first envelope
    // and opens another. Neither is something the agent asked for.
    ask("http://127.0.0.1:18402", &key).await?;

    let second = store.newest().await?.ok_or("no envelope")?;
    assert_ne!(second, first, "the envelope never rolled");
    assert!(
        daemon.index().is_spent(first).await?,
        "the first envelope was not settled on chain"
    );
    assert!(
        held(&provider, paid_to).await? > paid,
        "the seller was not paid"
    );
    assert!(
        !store.spendable(asset, now()).await?.is_empty(),
        "the change never came back spendable"
    );
    running.abort();
    Ok(())
}

/// The buyer walks away mid-session. The seller closes it alone, and the
/// buyer picks the change up when it returns.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a deployed pool, a prover and an upstream"]
async fn the_seller_closes_what_the_buyer_abandoned() -> Fallible {
    watching();
    let state = tempfile::tempdir()?;
    let gateway = "http://127.0.0.1:18411";
    seller("127.0.0.1:18411", state.path())?;
    up(&format!("{gateway}/v1/catalogue")).await?;

    // Room for more than one call, so what ends this session is the
    // deadline rather than the ceiling.
    let estimate = offered(gateway)
        .await?
        .rates()
        .most(asking().as_bytes(), CAP);
    let database = format!(
        "sqlite://{}?mode=rwc",
        state.path().join("veil.db").display()
    );
    let provider = ProviderBuilder::new().connect(&var("RPC_URL")?).await?;
    let config = buying(
        "127.0.0.1:18412",
        gateway,
        &database,
        state.path(),
        estimate * 4,
    )?;
    let note = deposit(&config, estimate * 4).await?;

    let asset = pool()?.asset();
    let paid_to: Address = var("VEIL_SELLER")?.parse()?;
    let paid = held(&provider, paid_to).await?;

    let daemon = Arc::new(Daemon::start(config).await?);
    let running = started(&daemon);
    placed(&daemon, &running, note).await?;
    let key = ledger(&daemon)?.issue("test", None, None, now()).await?.key;
    ask("http://127.0.0.1:18412", &key).await?;

    let store = SqliteStore::open(&database).await?;
    let opened = store.newest().await?.ok_or("no envelope was opened")?;

    // Gone, and holding nothing: the engine's state has to be released
    // before the same daemon can be started again.
    running.abort();
    let _ = running.await;
    drop(daemon);

    // Nobody is left to settle this but the seller.
    let pool = veil402_gateway::adapters::connect(&var("RPC_URL")?, var("POOL")?.parse()?).await?;
    closed(&pool, opened).await?;
    assert!(
        held(&provider, paid_to).await? > paid,
        "the seller closed the session without being paid"
    );

    // The buyer comes back to a settlement it did not make, and writes the
    // change down from the chain alone.
    let config = buying(
        "127.0.0.1:18413",
        gateway,
        &database,
        state.path(),
        estimate * 4,
    )?;
    let daemon = Arc::new(Daemon::start(config).await?);
    let running = started(&daemon);
    spendable(&store, &running, asset).await?;

    let key = ledger(&daemon)?.issue("back", None, None, now()).await?.key;
    ask("http://127.0.0.1:18413", &key).await?;
    assert_ne!(
        store.newest().await?.ok_or("no envelope")?,
        opened,
        "the daemon reopened the session the seller had closed"
    );
    running.abort();
    Ok(())
}
