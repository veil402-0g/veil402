//! The index is exercised against fakes, so everything here is the service's
//! own logic rather than a chain's or a database's.

mod common;

use std::sync::Arc;

use common::{Chain, Fallible, Memory, field};
use veil402_daemon::core::index::{IndexError, IndexService};

#[tokio::test]
async fn sync_indexes_in_leaf_order_and_agrees_with_the_pool() -> Fallible {
    let index = IndexService::load(Arc::new(Chain::new(9)), Arc::new(Memory::default())).await?;

    let health = index.sync().await?;
    assert_eq!(health.leaves, 9);
    assert_eq!(index.leaves().await, 9);
    assert!(index.is_current().await?);
    Ok(())
}

/// The pool keeps every root it has reached, so an index that is behind can
/// still build a spend. Only being wrong matters.
#[tokio::test]
async fn being_behind_is_not_an_error() -> Fallible {
    let store = Arc::new(Memory::default());
    IndexService::load(Arc::new(Chain::new(4)), store.clone())
        .await?
        .sync()
        .await?;

    // Two more notes land that this index has not seen.
    let index = IndexService::load(Arc::new(Chain::new(6)), store).await?;
    let health = index.health(0).await?;

    assert!(!index.is_current().await?, "it is plainly behind");
    assert_eq!(health.leaves, 4);
    assert_eq!(index.witness(&[3]).await?.paths[0].index, 3);
    Ok(())
}

/// A dropped log is caught where it happens, naming the leaf.
#[tokio::test]
async fn a_skipped_leaf_is_refused() {
    let index = IndexService::load(
        Arc::new(Chain::withholding(5, 2)),
        Arc::new(Memory::default()),
    )
    .await
    .expect("load");

    assert!(matches!(
        index.sync().await,
        Err(IndexError::Gap { expected: 2, got: 3 })
    ));
}

/// A commitment with the right index but the wrong value only shows up in the
/// root, which is why the root is checked every pass.
#[tokio::test]
async fn a_wrong_commitment_is_a_divergence() {
    let index = IndexService::load(
        Arc::new(Chain::corrupting(5, 2)),
        Arc::new(Memory::default()),
    )
    .await
    .expect("load");

    assert!(matches!(
        index.sync().await,
        Err(IndexError::Diverged { .. })
    ));
}

/// The chain only answers a fixed window, so a restart has to come back from
/// the store alone — and from its nodes, without hashing the history again.
#[tokio::test]
async fn a_restart_restores_the_same_tree_from_stored_nodes() -> Fallible {
    let store = Arc::new(Memory::default());
    let first = IndexService::load(Arc::new(Chain::new(6)), store.clone()).await?;
    first.sync().await?;
    let root = first.root().await?;
    drop(first);

    let restarted = IndexService::load(Arc::new(Chain::new(6)), store).await?;
    assert_eq!(restarted.root().await?, root);
    assert_eq!(restarted.leaves().await, 6);
    Ok(())
}

/// A refused write must leave the tree where the store is, or the next path it
/// hands out authenticates against a root nobody holds.
#[tokio::test]
async fn a_refused_write_leaves_the_tree_where_the_store_is() -> Fallible {
    let store = Arc::new(Memory::default());
    let index = IndexService::load(Arc::new(Chain::new(4)), store.clone()).await?;
    index.sync().await?;
    let root = index.root().await?;

    store.refuse();
    let index = IndexService::load(Arc::new(Chain::new(6)), store).await?;
    assert!(index.sync().await.is_err());

    assert_eq!(index.leaves().await, 4, "the tree ran ahead of the store");
    assert_eq!(index.root().await?, root);
    Ok(())
}

/// Both spends in a transaction share one circuit root, so the paths have to be
/// read from one tree state.
#[tokio::test]
async fn a_witness_carries_one_root_for_every_leaf() -> Fallible {
    let index = IndexService::load(Arc::new(Chain::new(5)), Arc::new(Memory::default())).await?;
    index.sync().await?;

    let witness = index.witness(&[1, 4]).await?;
    assert_eq!(witness.root, index.root().await?);
    assert_eq!(witness.paths.len(), 2);
    assert!(matches!(
        index.witness(&[1, 5]).await,
        Err(IndexError::Unknown(5))
    ));
    Ok(())
}

/// What a payee asks: is the note I was paid to in the tree, and where.
#[tokio::test]
async fn a_commitment_resolves_to_its_leaf() -> Fallible {
    let index = IndexService::load(Arc::new(Chain::new(5)), Arc::new(Memory::default())).await?;
    index.sync().await?;

    assert_eq!(index.leaf_of(field(3)).await?, Some(2));
    assert_eq!(index.leaf_of(field(99)).await?, None);
    assert_eq!(index.notes(2, 2).await?.len(), 2);
    assert_eq!(index.notes(2, 2).await?[0].leaf_index, 2);
    Ok(())
}

/// What a session asks before it reserves a note: has this nullifier already
/// been seen on chain.
#[tokio::test]
async fn nullifiers_are_indexed() -> Fallible {
    let index = IndexService::load(
        Arc::new(Chain::spending(3, &[77])),
        Arc::new(Memory::default()),
    )
    .await?;
    index.sync().await?;

    assert!(index.is_spent(field(77)).await?);
    assert!(!index.is_spent(field(78)).await?);
    Ok(())
}
