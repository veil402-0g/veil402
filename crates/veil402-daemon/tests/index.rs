//! The index is exercised against fakes, so everything here is the service's
//! own logic rather than a chain's or a database's.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use veil402_daemon::core::index::{
    Batch, ChainError, Commitment, CommitmentSource, IndexError, IndexService, IndexStore,
    Nullified, StoreError,
};
use veil402_sdk::{Field, StoredNode, Tree};

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn field(value: u64) -> Field {
    Field::from(value)
}

/// A pool holding `held` commitments, one per block. `withheld` is a leaf the
/// source refuses to report while the root still counts it, which is what a
/// missed event looks like.
struct Chain {
    held: Vec<Field>,
    spent: Vec<Field>,
    withheld: Option<usize>,
    corrupted: Option<usize>,
}

impl Chain {
    fn new(count: u64) -> Self {
        Self {
            held: (1..=count).map(field).collect(),
            spent: Vec::new(),
            withheld: None,
            corrupted: None,
        }
    }

    /// A pool that has also recorded nullifiers.
    fn spending(count: u64, spent: &[u64]) -> Self {
        Self {
            spent: spent.iter().copied().map(field).collect(),
            ..Self::new(count)
        }
    }

    /// Skips a leaf entirely, which is what a dropped log looks like.
    fn withholding(count: u64, leaf: usize) -> Self {
        Self {
            withheld: Some(leaf),
            ..Self::new(count)
        }
    }

    /// Reports every leaf but one with the wrong value, so only the root
    /// notices.
    fn corrupting(count: u64, leaf: usize) -> Self {
        Self {
            corrupted: Some(leaf),
            ..Self::new(count)
        }
    }

    /// Every root the pool has passed through, which is what `knownRoot`
    /// remembers.
    fn roots(&self) -> Result<Vec<Field>, ChainError> {
        let mut tree = Tree::new();
        self.held
            .iter()
            .map(|commitment| {
                tree.insert(*commitment)
                    .map_err(|_| ChainError::InvalidData)?;
                tree.root().map_err(|_| ChainError::InvalidData)
            })
            .collect()
    }

    fn height(&self) -> Result<u64, ChainError> {
        u64::try_from(self.held.len()).map_err(|_| ChainError::InvalidData)
    }
}

#[async_trait]
impl CommitmentSource for Chain {
    async fn since(&self, block: u64) -> Result<Batch, ChainError> {
        let mut commitments = Vec::new();
        for (index, commitment) in self.held.iter().enumerate() {
            let leaf_index = u32::try_from(index).map_err(|_| ChainError::InvalidData)?;
            let at = u64::from(leaf_index) + 1;
            if at > block && self.withheld != Some(index) {
                commitments.push(Commitment {
                    leaf_index,
                    commitment: if self.corrupted == Some(index) {
                        field(u64::MAX)
                    } else {
                        *commitment
                    },
                    note: index.to_be_bytes().to_vec(),
                    block: at,
                });
            }
        }
        let nullifiers = self
            .spent
            .iter()
            .filter(|_| block == 0)
            .map(|nullifier| Nullified {
                nullifier: *nullifier,
                block: 1,
            })
            .collect();
        Ok(Batch {
            commitments,
            nullifiers,
            through: self.height()?,
        })
    }

    async fn is_known_root(&self, root: Field) -> Result<bool, ChainError> {
        Ok(self.roots()?.contains(&root))
    }

    async fn root(&self) -> Result<Field, ChainError> {
        self.roots()?.pop().ok_or(ChainError::InvalidData)
    }
}

/// Mirrors the real store: nodes are upserted by position and come back in
/// order, and a refused write must leave the service's tree untouched.
#[derive(Default)]
struct Memory {
    commitments: Mutex<Vec<Commitment>>,
    nodes: Mutex<BTreeMap<(u8, u32), Field>>,
    cursor: Mutex<u64>,
    spent: Mutex<HashSet<Field>>,
    refusing: AtomicBool,
}

impl Memory {
    fn refuse(&self) {
        self.refusing.store(true, Ordering::Relaxed);
    }

    fn held<T>(lock: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, StoreError> {
        lock.lock().map_err(|_| StoreError::Unavailable)
    }
}

#[async_trait]
impl IndexStore for Memory {
    async fn cursor(&self) -> Result<u64, StoreError> {
        Ok(*Self::held(&self.cursor)?)
    }

    async fn nodes(&self) -> Result<Vec<StoredNode>, StoreError> {
        Ok(Self::held(&self.nodes)?
            .iter()
            .map(|((level, index), node)| StoredNode {
                level: *level,
                index: *index,
                node: *node,
            })
            .collect())
    }

    async fn append(&self, batch: &Batch, nodes: &[StoredNode]) -> Result<(), StoreError> {
        if self.refusing.load(Ordering::Relaxed) {
            return Err(StoreError::Unavailable);
        }
        Self::held(&self.commitments)?.extend(batch.commitments.iter().cloned());
        Self::held(&self.spent)?.extend(batch.nullifiers.iter().map(|spent| spent.nullifier));
        let mut held = Self::held(&self.nodes)?;
        for node in nodes {
            held.insert((node.level, node.index), node.node);
        }
        *Self::held(&self.cursor)? = batch.through;
        Ok(())
    }

    async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, StoreError> {
        Ok(Self::held(&self.commitments)?
            .iter()
            .find(|held| held.commitment == commitment)
            .map(|held| held.leaf_index))
    }

    async fn is_spent(&self, nullifier: Field) -> Result<bool, StoreError> {
        Ok(Self::held(&self.spent)?.contains(&nullifier))
    }

    async fn notes(&self, from: u32, limit: u32) -> Result<Vec<Commitment>, StoreError> {
        Ok(Self::held(&self.commitments)?
            .iter()
            .filter(|held| held.leaf_index >= from)
            .take(limit as usize)
            .cloned()
            .collect())
    }
}

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
