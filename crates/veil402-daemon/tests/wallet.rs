//! Note selection, splitting and reconciliation.
//!
//! The store is the real one on an in-memory database rather than a fake: the
//! claim that stops two sessions taking the same note is a conditional UPDATE,
//! and a mock of it would only ever agree with itself.

use std::sync::Arc;

use veil402_daemon::{
    adapters::SqliteStore,
    core::{
        index::{Batch, Commitment, IndexStore, Locked, Nullified},
        wallet::{Held, Minted, NoteStore, WalletError, WalletService, select, split},
    },
};
use veil402_sdk::{Field, Owner, Secret};

type Fallible = Result<(), Box<dyn std::error::Error>>;

/// The pool holds one asset, and which one is never the question here.
const ASSET: Field = Field::ZERO;

fn field(value: u64) -> Field {
    Field::from(value)
}

async fn store() -> Result<Arc<SqliteStore>, Box<dyn std::error::Error>> {
    Ok(Arc::new(SqliteStore::open("sqlite::memory:").await?))
}

fn wallet(store: &Arc<SqliteStore>, owner: Owner) -> WalletService {
    WalletService::new(store.clone(), store.clone(), owner)
}

/// Puts `count` commitments in the tree, `field(1)` first, so a note minted
/// against `field(n)` resolves to leaf `n - 1`.
async fn emit(store: &SqliteStore, count: u64) -> Fallible {
    let batch = Batch {
        commitments: (0..count)
            .map(|index| Commitment {
                leaf_index: u32::try_from(index).unwrap_or_default(),
                commitment: field(index + 1),
                note: Vec::new(),
                block: index + 1,
            })
            .collect(),
        through: count.max(1),
        ..Batch::default()
    };
    Ok(store.append(&batch, &[]).await?)
}

/// What the pool records after a lock or a settlement.
async fn observe(store: &SqliteStore, nullifiers: &[Field], locks: &[(Field, u64)]) -> Fallible {
    let batch = Batch {
        nullifiers: nullifiers
            .iter()
            .map(|nullifier| Nullified {
                nullifier: *nullifier,
                block: 1,
            })
            .collect(),
        locks: locks
            .iter()
            .map(|(nullifier, deadline)| Locked {
                nullifier: *nullifier,
                deadline: *deadline,
                block: 1,
            })
            .collect(),
        through: 1,
        ..Batch::default()
    };
    Ok(store.append(&batch, &[]).await?)
}

fn minted(commitment: Field, value: u64) -> Result<Minted, Box<dyn std::error::Error>> {
    Ok(Minted {
        commitment,
        value,
        asset: ASSET,
        random: Secret::random()?,
    })
}

fn holding(values: &[u64]) -> Result<Vec<Held>, Box<dyn std::error::Error>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let index = u32::try_from(index)?;
            Ok(Held {
                commitment: field(u64::from(index) + 1),
                leaf: index,
                nullifier: field(u64::from(index) + 1_000),
                value: *value,
                asset: ASSET,
                random: Secret::random()?,
            })
        })
        .collect()
}

#[test]
fn select_takes_the_smallest_note_that_covers_the_session() -> Fallible {
    let held = holding(&[10, 100, 30, 50])?;

    assert_eq!(select(&held, 30)?, 2, "an exact cover is still the smallest");
    assert_eq!(select(&held, 31)?, 3);
    assert_eq!(select(&held, 1)?, 0);
    Ok(())
}

/// Which failure it is decides what the operator has to do, so they are not
/// the same error.
#[test]
fn a_shortfall_says_whether_a_split_would_fix_it() -> Fallible {
    let held = holding(&[10, 20, 30])?;

    assert!(matches!(
        select(&held, 45),
        Err(WalletError::Fragmented {
            balance: 60,
            notes: 3,
            ..
        })
    ));
    assert!(matches!(
        select(&held, 61),
        Err(WalletError::Insufficient { balance: 60, .. })
    ));
    Ok(())
}

#[test]
fn split_fills_the_output_slot_a_settlement_would_waste() {
    assert!(split(0, 10).is_empty(), "a zero note cannot be sent");
    assert_eq!(split(5, 10), vec![5], "too small to leave a session behind");
    assert_eq!(split(19, 10), vec![19]);
    assert_eq!(split(20, 10), vec![10, 10]);
    assert_eq!(split(75, 10), vec![10, 65]);
    assert_eq!(split(75, 0), vec![75], "zero target never splits");
}

#[tokio::test]
async fn reconcile_places_a_note_once_the_index_has_seen_it() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    emit(&store, 3).await?;
    // field(2) is the pool's second commitment, so it sits at leaf 1.
    wallet.remember(&[minted(field(2), 70)?], 0).await?;

    assert_eq!(wallet.reconcile(0).await?.placed, 1);
    assert_eq!(store.spendable(ASSET, 0).await?[0].leaf, 1);
    assert_eq!(wallet.reconcile(0).await?.placed, 0, "placed only once");
    Ok(())
}

/// One pass places and retires, which is what a restart sees: the note was
/// spent while the daemon was away.
#[tokio::test]
async fn reconcile_retires_a_note_the_pool_has_nullified() -> Fallible {
    let owner = Owner::random()?;
    // Only the owner can name the nullifier of its own note at leaf 0.
    let nullifier = owner.nullifier(0)?;
    let store = store().await?;
    let wallet = wallet(&store, owner);
    emit(&store, 1).await?;
    observe(&store, &[nullifier], &[]).await?;
    wallet.remember(&[minted(field(1), 40)?], 0).await?;

    let pass = wallet.reconcile(0).await?;
    assert_eq!((pass.placed, pass.spent), (1, 1));
    assert!(store.spendable(ASSET, 0).await?.is_empty());
    Ok(())
}

/// Two sessions that read the same free note must not both get it, so the
/// claim is a conditional write rather than a read followed by one.
#[tokio::test]
async fn only_one_session_can_claim_a_note() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    emit(&store, 1).await?;
    wallet.remember(&[minted(field(1), 500)?], 0).await?;
    wallet.reconcile(0).await?;

    let note = wallet.reserve(ASSET, 100, 0).await?;
    assert_eq!(note.value, 500);
    assert!(
        !store.hold(note.commitment, 0, 999).await?,
        "a second claim on a held note has to lose"
    );
    assert!(matches!(
        wallet.reserve(ASSET, 100, 0).await,
        Err(WalletError::Insufficient { .. })
    ));
    Ok(())
}

/// The question a failed lock asks. Nothing sweeps it up: the hold `reserve`
/// took is short on purpose, and an escrow that never arrives never extends
/// it.
#[tokio::test]
async fn a_lock_that_never_lands_frees_the_note_by_itself() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    emit(&store, 1).await?;
    wallet.remember(&[minted(field(1), 500)?], 0).await?;
    wallet.reconcile(0).await?;
    wallet.reserve(ASSET, 100, 0).await?;

    assert!(store.spendable(ASSET, 60).await?.is_empty(), "still held");
    assert_eq!(wallet.reconcile(200).await?.escrowed, 0, "no escrow to find");
    assert_eq!(
        store.spendable(ASSET, 200).await?.len(),
        1,
        "the hold lapsed and the note came back"
    );
    Ok(())
}

/// When the lock does land, the pool's own deadline replaces the short hold,
/// so the note stays out of circulation for the whole session.
#[tokio::test]
async fn an_escrow_extends_the_hold_to_its_own_deadline() -> Fallible {
    let owner = Owner::random()?;
    let nullifier = owner.nullifier(0)?;
    let store = store().await?;
    let wallet = wallet(&store, owner);
    emit(&store, 1).await?;
    wallet.remember(&[minted(field(1), 500)?], 0).await?;
    wallet.reconcile(0).await?;
    wallet.reserve(ASSET, 100, 0).await?;
    // The lock the reservation was taken for lands.
    observe(&store, &[], &[(nullifier, 5_000)]).await?;

    assert_eq!(wallet.reconcile(10).await?.escrowed, 1);
    assert_eq!(wallet.reconcile(10).await?.escrowed, 0, "extended only once");
    assert!(
        store.spendable(ASSET, 200).await?.is_empty(),
        "the short hold would have lapsed by now"
    );
    // The pool frees it a grace window after the deadline, and so do we.
    assert!(store.spendable(ASSET, 5_050).await?.is_empty());
    assert_eq!(store.spendable(ASSET, 5_061).await?.len(), 1);
    Ok(())
}

/// A row that never gets a leaf is the only copy of what makes its note
/// spendable, so it is counted and left alone.
#[tokio::test]
async fn a_note_that_never_lands_is_counted_not_deleted() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    emit(&store, 1).await?;
    wallet.remember(&[minted(field(9), 40)?], 0).await?;

    assert_eq!(wallet.reconcile(0).await?.stale, 0, "not stale yet");
    assert_eq!(wallet.reconcile(7_200).await?.stale, 1);
    assert_eq!(store.unplaced().await?.len(), 1, "still on record");
    Ok(())
}
