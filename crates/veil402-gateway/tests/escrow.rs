//! A seller does no work against a reservation the pool does not hold.
//!
//! Every other suite is an honest buyer, which is exactly why none of them
//! catches this: the delegation arithmetic all checks out, and the lie is
//! about whether any of it is on chain.

mod support;

use std::{path::Path, sync::Arc};

use alloy::primitives::Address;
use veil402_gateway::{
    app::{Config, assemble},
    core::{MeterError, MeteringService},
};
use veil402_sdk::{
    Note, Secret, SessionKeys, Tree,
    evm::{PoolClient, Session},
    session::{Handover, Keyring, Recipient, RootSeed, Sealed},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

const ROOT: [u8; 32] = [5; 32];
const DEADLINE: u64 = 1_900_000_000;
const VALUE: u64 = 1_000;

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

fn signer() -> Result<Address, Box<dyn std::error::Error>> {
    Ok(Keyring::new(RootSeed::from_bytes(ROOT)).signer()?.address())
}

fn gateway(
    state: &Path,
    escrows: Arc<support::Pool>,
) -> Result<Arc<MeteringService>, Box<dyn std::error::Error>> {
    Ok(assemble(
        &Config {
            vouched: std::collections::HashMap::new(),
            fee_cap: 20_000_000_000_000,
            pool: pool()?,
            root: ROOT,
            worker: "/nonexistent/worker".into(),
            artifacts: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../prover/veil402-gnark/artifacts/transaction-v4"),
            seller: seller(),
            state: state.join("sessions.redb"),
            bind: "127.0.0.1:0".parse()?,
            upstream: "http://127.0.0.1:0".to_owned(),
            upstream_key: "sk-test".to_owned(),
            trust: "private".to_owned(),
            scale: 1,
            margin_bps: 0,
            ceiling: 1_024,
            signer: "0x0101010101010101010101010101010101010101010101010101010101010101".to_owned(),
            multicall: alloy::primitives::Address::repeat_byte(0xCA),
            wallet_state: std::path::PathBuf::from("/nonexistent/wallet.redb"),
            confirmations: 0,
            rpc: "http://127.0.0.1:0".to_owned(),
        },
        escrows,
        Arc::new(support::Upstream { frames: 2, breaks: false, ..support::Upstream::default() }),
    )?)
}

struct Delegation {
    sealed: Sealed,
    session: Session,
}

/// A delegation whose arithmetic is impeccable: the seed really owns the
/// note, and the path really reaches the root. The only question left is
/// whether the pool has ever heard of any of it.
fn delegate(service: &MeteringService) -> Result<Delegation, Box<dyn std::error::Error>> {
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
        seller: seller(),
        deadline: DEADLINE,
    };
    let agreed = service.agree(&session)?;
    let handover = Handover {
        seed,
        value: VALUE,
        asset: client.asset(),
        leaf,
        siblings: witness.paths[0].siblings,
        root: witness.root,
        change_seed: Secret::random()?,
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
    Ok(Delegation { sealed, session })
}

async fn accepted(pool_state: support::Pool) -> Result<Result<(), MeterError>, Box<dyn std::error::Error>> {
    let state = tempfile::tempdir()?;
    let service = gateway(state.path(), Arc::new(pool_state))?;
    let delegation = delegate(&service)?;
    Ok(service
        .accept(&delegation.sealed, delegation.session)
        .await)
}

/// The free-work attack: a buyer that locked nothing.
#[tokio::test]
async fn a_session_with_no_escrow_is_refused() -> Fallible {
    assert!(matches!(
        accepted(support::Pool::empty()).await?,
        Err(MeterError::Unreserved(_))
    ));
    Ok(())
}

/// A reservation made against somebody else's gateway pays somebody else.
#[tokio::test]
async fn an_escrow_for_another_enclave_is_refused() -> Fallible {
    assert!(matches!(
        accepted(support::Pool::backing(
            Address::repeat_byte(0xAA),
            seller(),
            DEADLINE
        ))
        .await?,
        Err(MeterError::Unreserved(_))
    ));
    Ok(())
}

/// A tree the buyer made up. The path reaches the root it names, the seed
/// owns the note — and the pool has never seen any of it, so the settlement
/// would revert and the work would be a gift.
#[tokio::test]
async fn a_root_the_pool_never_reached_is_refused() -> Fallible {
    assert!(matches!(
        accepted(support::Pool::backing(signer()?, seller(), DEADLINE).forgetting_roots()).await?,
        Err(MeterError::Unreserved(_))
    ));
    Ok(())
}

/// An escrow for another seller, or one already spent, is not ours to serve.
#[tokio::test]
async fn an_escrow_that_does_not_match_the_session_is_refused() -> Fallible {
    assert!(matches!(
        accepted(support::Pool::backing(signer()?, Address::repeat_byte(0x11), DEADLINE)).await?,
        Err(MeterError::Unreserved(_))
    ));
    assert!(matches!(
        accepted(support::Pool::backing(signer()?, seller(), DEADLINE).settled()).await?,
        Err(MeterError::Unreserved(_))
    ));
    Ok(())
}

/// And the honest one still works, or the check would be useless.
#[tokio::test]
async fn an_escrow_the_pool_really_holds_is_accepted() -> Fallible {
    assert!(
        accepted(support::Pool::backing(signer()?, seller(), DEADLINE))
            .await?
            .is_ok()
    );
    Ok(())
}
