//! Note selection, splitting and reconciliation.
//!
//! The store is the real one on an in-memory database rather than a fake: the
//! claim that stops two sessions taking the same note is a conditional UPDATE,
//! and a mock of it would only ever agree with itself.

use std::sync::Arc;

use veil402_daemon::{
    adapters::SqliteStore,
    core::{
        index::{Batch, Commitment, IndexStore, Locked, Nullified, Settled},
        envelope::SubmissionStore,
        wallet::{Held, Minted, NoteStore, Recovered, WalletError, WalletService, choose},
    },
};
use veil402_sdk::{Field, Owner, Secret, SessionKeys};

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
                seller: alloy::primitives::Address::repeat_byte(0x5E),
                enclave: alloy::primitives::Address::repeat_byte(0xE1),
                deadline: *deadline,
                block: 1,
            })
            .collect(),
        through: 1,
        ..Batch::default()
    };
    Ok(store.append(&batch, &[]).await?)
}

/// Puts specific commitments in the tree from `leaf` onward, for notes whose
/// values the test did not choose.
async fn place(store: &SqliteStore, commitments: &[Field], leaf: u32) -> Fallible {
    let batch = Batch {
        commitments: commitments
            .iter()
            .enumerate()
            .map(|(offset, commitment)| Commitment {
                leaf_index: leaf + u32::try_from(offset).unwrap_or_default(),
                commitment: *commitment,
                note: Vec::new(),
                block: 3,
            })
            .collect(),
        through: 3,
        ..Batch::default()
    };
    Ok(store.append(&batch, &[]).await?)
}

/// What the pool records when a seller settles: the escrow it held, the
/// nullifier it spends, and the amount only the calldata carried.
async fn settled(store: &SqliteStore, nullifier: Field, amount: u64) -> Fallible {
    let batch = Batch {
        nullifiers: vec![Nullified {
            nullifier,
            block: 2,
        }],
        locks: vec![Locked {
            nullifier,
            seller: alloy::primitives::Address::repeat_byte(0x5E),
            enclave: alloy::primitives::Address::repeat_byte(0xE1),
            deadline: 600,
            block: 1,
        }],
        settlements: vec![Settled {
            nullifier,
            amount,
            block: 2,
        }],
        through: 2,
        ..Batch::default()
    };
    Ok(store.append(&batch, &[]).await?)
}

fn minted(
    commitment: Field,
    value: u64,
    seed: Option<Secret>,
) -> Result<Minted, Box<dyn std::error::Error>> {
    Ok(Minted {
        commitment,
        value,
        asset: ASSET,
        random: Secret::random()?,
        seed,
    })
}

fn seeding(notes: &[(u64, bool)]) -> Result<Vec<Held>, Box<dyn std::error::Error>> {
    notes
        .iter()
        .enumerate()
        .map(|(leaf, (value, seeded))| {
            let leaf = u32::try_from(leaf)?;
            Ok(Held {
                commitment: field(u64::from(leaf)),
                leaf,
                nullifier: field(u64::from(leaf) + 1_000),
                value: *value,
                asset: ASSET,
                random: Secret::random()?,
                seed: seeded.then(Secret::random).transpose()?,
            })
        })
        .collect()
}

/// A seeded note is the one to want — it was minted for this and costs
/// nothing to hand over. Only when none of them covers the session is it
/// worth reseeding something, and then it may as well be the biggest.
#[test]
fn a_seeded_note_is_preferred_and_the_largest_is_the_fallback() -> Fallible {
    // 10 and 50 carry a seed; 100 and 30 are the wallet's own.
    let held = seeding(&[(10, true), (100, false), (30, false), (50, true)])?;

    assert_eq!(choose(&held, 30)?, 3, "the smallest seeded note that covers it");
    assert_eq!(choose(&held, 10)?, 0, "and smaller when a smaller one will do");
    assert_eq!(
        choose(&held, 51)?,
        1,
        "no seeded note covers it, so the largest is worth reseeding"
    );

    // Nothing seeded at all is the first envelope, before any settlement.
    let cold = seeding(&[(10, false), (100, false)])?;
    assert_eq!(choose(&cold, 30)?, 1);
    Ok(())
}

/// With no size to meet, the only way to have no note is to hold none.
#[test]
fn an_empty_wallet_is_the_only_refusal() {
    assert!(matches!(choose(&[], 1), Err(WalletError::Empty)));
}

#[tokio::test]
async fn reconcile_places_a_note_once_the_index_has_seen_it() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    emit(&store, 3).await?;
    // field(2) is the pool's second commitment, so it sits at leaf 1.
    wallet.remember(&[minted(field(2), 70, None)?], 0).await?;

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
    wallet.remember(&[minted(field(1), 40, None)?], 0).await?;

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
    wallet.remember(&[minted(field(1), 500, None)?], 0).await?;
    wallet.reconcile(0).await?;

    let note = wallet.reserve(ASSET, 100, 0).await?;
    assert_eq!(note.value, 500);
    assert!(
        !store.hold(note.commitment, 0, 999).await?,
        "a second claim on a held note has to lose"
    );
    assert!(matches!(
        wallet.reserve(ASSET, 100, 0).await,
        Err(WalletError::Empty)
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
    wallet.remember(&[minted(field(1), 500, None)?], 0).await?;
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
    wallet.remember(&[minted(field(1), 500, None)?], 0).await?;
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
    wallet.remember(&[minted(field(9), 40, None)?], 0).await?;

    assert_eq!(wallet.reconcile(0).await?.stale, 0, "not stale yet");
    assert_eq!(wallet.reconcile(7_200).await?.stale, 1);
    assert_eq!(store.unplaced().await?.len(), 1, "still on record");
    Ok(())
}

/// A session note belongs to its own owner. Deriving its nullifier from this
/// wallet's key writes one the pool will never record, and the note is then
/// offered forever.
#[tokio::test]
async fn a_session_note_reveals_its_own_nullifier() -> Fallible {
    let wallet_owner = Owner::random()?;
    let seed = Secret::random()?;
    // field(1) is the first commitment, so the note lands at leaf 0.
    let session = SessionKeys::from_seed(&seed)?.owner.nullifier(0)?;
    assert_ne!(session, wallet_owner.nullifier(0)?, "the test would prove nothing");

    let store = store().await?;
    let wallet = wallet(&store, wallet_owner);
    emit(&store, 1).await?;
    wallet.remember(&[minted(field(1), 500, Some(seed))?], 0).await?;
    wallet.reconcile(0).await?;

    assert_eq!(store.outstanding().await?[0].nullifier, session);

    // And the pool recording that nullifier retires it.
    observe(&store, &[session], &[]).await?;
    assert_eq!(wallet.reconcile(0).await?.spent, 1);
    assert!(store.spendable(ASSET, 0).await?.is_empty());
    Ok(())
}

/// Both kinds coexist: one pass has to use the right key for each.
#[tokio::test]
async fn session_and_wallet_notes_reconcile_together() -> Fallible {
    let wallet_owner = Owner::random()?;
    let seed = Secret::random()?;
    let store = store().await?;
    let wallet = wallet(&store, wallet_owner);
    emit(&store, 2).await?;
    wallet
        .remember(
            &[
                minted(field(1), 100, Some(seed))?,
                minted(field(2), 200, None)?,
            ],
            0,
        )
        .await?;

    assert_eq!(wallet.reconcile(0).await?.placed, 2);
    assert_eq!(store.spendable(ASSET, 0).await?.len(), 2);
    Ok(())
}

/// A seller settles while the daemon is elsewhere. What comes back has to be
/// derivable from the seed it kept and the figure the pool recorded, because
/// those are the only two things it has.
#[tokio::test]
async fn settled_change_is_recovered_from_the_chain_alone() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    let session = Secret::random()?;
    let change = Secret::random()?;

    wallet
        .remember(&[minted(field(1), 1_000, Some(Secret::from_bytes(session.to_bytes())?))?], 0)
        .await?;
    emit(&store, 1).await?;
    wallet.reconcile(0).await?;
    let nullifier = SessionKeys::from_seed(&session)?.owner.nullifier(0)?;

    settled(&store, nullifier, 300).await?;
    assert!(matches!(
        wallet.recover(nullifier, &change, 0).await?,
        Recovered::Change(_)
    ));

    // The whole remainder, seeded, so the next envelope opens on it without
    // a reseed. Nothing is carved off for the wallet's own key: there is no
    // size to carve to, and the seed already reaches this note.
    let recovered = store.unplaced().await?;
    assert_eq!(recovered.len(), 1);
    assert!(recovered[0].seed.is_some(), "the change came back undelegatable");

    let commitments: Vec<Field> = recovered.iter().map(|note| note.commitment).collect();
    place(&store, &commitments, 1).await?;
    wallet.reconcile(0).await?;

    let spendable: Vec<u64> = store
        .spendable(ASSET, 0)
        .await?
        .iter()
        .map(|note| note.value)
        .collect();
    assert_eq!(spendable, vec![700], "the settled note's change is spendable");
    Ok(())
}

/// A seller paid in a note publishes no amount, and guessing one would mint
/// commitments the chain never saw. Finding that change needs note discovery.
#[tokio::test]
async fn a_private_settlement_recovers_nothing() -> Fallible {
    let store = store().await?;
    let wallet = wallet(&store, Owner::random()?);
    let session = Secret::random()?;

    wallet
        .remember(&[minted(field(1), 1_000, Some(Secret::from_bytes(session.to_bytes())?))?], 0)
        .await?;
    emit(&store, 1).await?;
    wallet.reconcile(0).await?;
    let nullifier = SessionKeys::from_seed(&session)?.owner.nullifier(0)?;

    settled(&store, nullifier, 0).await?;
    assert_eq!(
        wallet.recover(nullifier, &Secret::random()?, 0).await?,
        Recovered::Nothing
    );
    assert!(store.unplaced().await?.is_empty(), "nothing was invented");
    Ok(())
}

/// A resubmission after a crash must adopt the call already in flight. Were
/// the second write to win, the daemon would track one handle for a
/// transaction it had sent twice, at two nonces.
#[tokio::test]
async fn a_resubmission_keeps_the_first_handle() -> Fallible {
    let store = store().await?;

    store.track(field(1), b"first").await?;
    store.track(field(1), b"second").await?;

    assert_eq!(store.handle(field(1)).await?, Some(b"first".to_vec()));
    assert_eq!(store.handle(field(2)).await?, None, "an unsent key has no handle");
    Ok(())
}
