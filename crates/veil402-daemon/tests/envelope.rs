//! Opening an envelope against a reserved note.
//!
//! The enclave here is real enough to matter: it derives its keys from a root
//! seed and vouches for them the way the daemon checks, so a delegation that
//! would not seal in production does not seal here either. Proving is not —
//! the split and settle paths need a worker and a chain, and M1 is where they
//! are exercised.

mod common;

use alloy::primitives::Address;
use common::{CEILING, Fallible, WINDOW, fixture, pool, asking, seller};
use futures_util::StreamExt;
use veil402_daemon::core::{
    envelope::{EnvelopeStore, Landed, Piece},
    wallet::NoteStore,
};

/// The change seed has to be on disk before the lock leaves, or a crash
/// between them reserves a note whose change nothing can ever identify.
#[tokio::test]
async fn an_envelope_records_its_change_seed_before_it_locks() -> Fallible {
    let fixture = fixture(Landed::Confirmed { block: 2 }, 2).await?;

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
    let fixture = fixture(Landed::Failed("AlreadyLocked".into()), 2).await?;

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
    let fixture = fixture(Landed::Confirmed { block: 2 }, 2).await?;

    assert!(fixture.service.open(seller(), CEILING, 3_601).await.is_err());
    assert!(fixture.chain.sent.lock().expect("sent").is_empty());
    Ok(())
}

/// An envelope that runs out does so for everybody at once. Without one
/// rollover between them, every request in flight would settle it and open
/// its own replacement — a proof, a lock and a note each.
#[tokio::test]
async fn one_rollover_serves_every_waiting_agent() -> Fallible {
    let fixture = fixture(Landed::Confirmed { block: 2 }, 2).await?;
    let service = fixture.service.clone();

    let asking: Vec<_> = (0..4)
        .map(|call| {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .dispatch(&asking(), &format!("call-{call}"))
                    .await
                    .map(|_| ())
            })
        })
        .collect();
    for asked in asking {
        asked.await??;
    }

    assert_eq!(
        fixture.chain.sent.lock().expect("sent").len(),
        1,
        "the envelope was opened more than once"
    );
    assert_eq!(fixture.enclave.accepted().len(), 1);
    Ok(())
}

/// The answer reaches the caller as it arrives. Collecting it first would
/// hold every token back until the last one was in.
#[tokio::test]
async fn an_answer_arrives_in_pieces() -> Fallible {
    let fixture = fixture(Landed::Confirmed { block: 2 }, 2).await?;

    let pieces: Vec<Piece> = fixture
        .service
        .dispatch(&asking(), "call")
        .await?
        .map(|piece| piece.expect("a piece refused"))
        .collect()
        .await;

    assert!(
        matches!(pieces.as_slice(), [Piece::Body(_), Piece::Body(_), Piece::Billed(_)]),
        "the pieces did not arrive as pieces, in order"
    );
    Ok(())
}

/// The seller closes a session itself when its deadline nears, so an
/// envelope can be spent without this daemon ever asking. The change is
/// ours either way, and nothing else goes looking for it.
#[tokio::test]
async fn change_from_a_settlement_we_did_not_make_is_recovered() -> Fallible {
    use veil402_daemon::core::index::{Batch, IndexStore, Locked, Nullified, Settled};

    let fixture = fixture(Landed::Confirmed { block: 2 }, 2).await?;
    let envelope = fixture.service.open(seller(), CEILING, WINDOW).await?;

    // The seller settled it while we were elsewhere: the nullifier is spent
    // and the pool published what it paid.
    fixture
        .store
        .append(
            &Batch {
                // The lock the chain saw first. A settlement is recorded
                // against it, so without it there is nothing to settle.
                locks: vec![Locked {
                    nullifier: envelope.nullifier,
                    seller: seller(),
                    enclave: Address::repeat_byte(0xE1),
                    deadline: envelope.deadline,
                    block: 2,
                }],
                nullifiers: vec![Nullified {
                    nullifier: envelope.nullifier,
                    block: 3,
                }],
                settlements: vec![Settled {
                    nullifier: envelope.nullifier,
                    amount: 400,
                    block: 3,
                }],
                through: 3,
                ..Batch::default()
            },
            &[],
        )
        .await?;

    fixture.service.recovered().await?;

    let change = fixture.store.unplaced().await?;
    assert_eq!(change.len(), 1, "the change was not written down");
    assert!(
        change[0].seed.is_some(),
        "the change came back with no seed to open it"
    );

    // Running again finds the same settlement and writes nothing new, so a
    // sync pass can do this every time without minting phantoms.
    fixture.service.recovered().await?;
    assert_eq!(fixture.store.unplaced().await?.len(), 1);
    Ok(())
}
