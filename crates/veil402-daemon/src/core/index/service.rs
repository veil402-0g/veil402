use std::sync::Arc;

use thiserror::Error;
use tokio::sync::RwLock;
use veil402_sdk::{Field, Tree, Witness};

use super::{Batch, ChainError, Commitment, CommitmentSource, Health, IndexStore, StoreError};

#[derive(Debug, Error)]
pub enum IndexError {
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The pool never held this root, so some commitment was missed and every
    /// path the index would hand out is wrong. Being merely behind is not this.
    #[error("the pool never held the root this index derives at {leaves} leaves")]
    Diverged { leaves: u32 },
    /// A leaf arrived out of order, which is cheaper to catch here than to let
    /// the root notice, and names the leaf.
    #[error("expected leaf {expected}, got {got}")]
    Gap { expected: u32, got: u32 },
    #[error("leaf {0} is not indexed")]
    Unknown(u32),
    #[error("the tree rejected a commitment")]
    Tree,
}

impl IndexError {
    /// Whether reading again could plausibly succeed.
    ///
    /// A divergence, a gap or a corrupt store cannot be read away: the index
    /// is wrong rather than behind, and every path it would hand out is wrong
    /// with it. An endpoint that is away is a different thing entirely.
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        matches!(
            self,
            Self::Chain(ChainError::Unavailable | ChainError::InvalidData)
                | Self::Store(StoreError::Unavailable)
        )
    }
}

/// The pool's tree, mirrored from the commitments it emitted.
///
/// The pool keeps only the roots it has reached; the siblings a spend needs
/// exist nowhere on chain, so anything that wants to spend more than its first
/// note has to carry the tree itself.
pub struct IndexService {
    source: Arc<dyn CommitmentSource>,
    store: Arc<dyn IndexStore>,
    tree: RwLock<Tree>,
}

/// Leaf order is the whole contract: one commitment out of place shifts every
/// sibling above it.
fn extend(tree: &mut Tree, commitments: &[Commitment]) -> Result<(), IndexError> {
    for held in commitments {
        let index = tree.insert(held.commitment).map_err(|_| IndexError::Tree)?;
        if index != held.leaf_index {
            return Err(IndexError::Gap {
                expected: index,
                got: held.leaf_index,
            });
        }
    }
    Ok(())
}

impl IndexService {
    /// Restores the tree from its stored nodes, never from the chain: the chain
    /// only answers a fixed window, so the store is the authority for anything
    /// older.
    ///
    /// # Errors
    ///
    /// Returns an [`IndexError`] when the store is unreadable or holds a gap.
    pub async fn load(
        source: Arc<dyn CommitmentSource>,
        store: Arc<dyn IndexStore>,
    ) -> Result<Self, IndexError> {
        let tree = Tree::load(&store.nodes().await?).map_err(|_| IndexError::Tree)?;
        Ok(Self {
            source,
            store,
            tree: RwLock::new(tree),
        })
    }

    /// Pulls everything new, stores it, and checks the pool recognises the
    /// result.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Diverged`] when the pool never held the resulting
    /// root, which means a commitment was missed.
    pub async fn sync(&self) -> Result<Health, IndexError> {
        let block = self.store.cursor().await?;
        let batch = self.source.since(block).await?;
        if batch.through > block {
            self.apply(&batch).await?;
        }
        self.health(batch.through.max(block)).await
    }

    /// The tree only moves once its nodes are durable, so a failed write leaves
    /// it exactly where the store is.
    async fn apply(&self, batch: &Batch) -> Result<(), IndexError> {
        let mut tree = self.tree.write().await;
        let nodes = match extend(&mut tree, &batch.commitments) {
            Ok(()) => tree.pending(),
            Err(error) => {
                tree.rollback();
                return Err(error);
            }
        };
        match self.store.append(batch, &nodes).await {
            Ok(()) => {
                tree.commit();
                Ok(())
            }
            Err(error) => {
                tree.rollback();
                Err(error.into())
            }
        }
    }

    /// Returns paths for `leaves`, all against one root.
    ///
    /// The circuit takes a single root, so paths read from different tree
    /// states cannot be proved together; taking them under one guard is what
    /// rules that out.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Unknown`] when the index holds no such leaf.
    pub async fn witness(&self, leaves: &[u32]) -> Result<Witness, IndexError> {
        let tree = self.tree.read().await;
        if let Some(leaf) = leaves.iter().find(|leaf| **leaf >= tree.len()) {
            return Err(IndexError::Unknown(*leaf));
        }
        tree.witness(leaves).map_err(|_| IndexError::Tree)
    }

    /// Returns the root the local tree derives.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Tree`] when the tree holds a non-canonical node.
    pub async fn root(&self) -> Result<Field, IndexError> {
        self.tree.read().await.root().map_err(|_| IndexError::Tree)
    }

    /// Returns how many commitments the index holds.
    pub async fn leaves(&self) -> u32 {
        self.tree.read().await.len()
    }

    /// Whether a nullifier has already been seen on chain.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Store`] when the store is unreadable.
    pub async fn is_spent(&self, nullifier: Field) -> Result<bool, IndexError> {
        Ok(self.store.is_spent(nullifier).await?)
    }

    /// Returns the leaf a commitment landed at, for a payee confirming its note
    /// is in the tree.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Store`] when the store is unreadable.
    pub async fn leaf_of(&self, commitment: Field) -> Result<Option<u32>, IndexError> {
        Ok(self.store.leaf_of(commitment).await?)
    }

    /// Returns commitments from `from`, for scanning ciphertexts to find owned
    /// notes.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Store`] when the store is unreadable.
    pub async fn notes(&self, from: u32, limit: u32) -> Result<Vec<Commitment>, IndexError> {
        Ok(self.store.notes(from, limit).await?)
    }

    /// Whether the index has caught up. Reporting only: being behind costs
    /// nothing, so this is never checked on the spend path.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Chain`] when the pool cannot be read.
    pub async fn is_current(&self) -> Result<bool, IndexError> {
        Ok(self.root().await? == self.source.root().await?)
    }

    /// Asks the pool the question it asks itself when a proof arrives, so a
    /// root that would be rejected is found before anything is proved.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Diverged`] when the pool never held this root.
    pub async fn health(&self, block: u64) -> Result<Health, IndexError> {
        let (leaves, root) = {
            let tree = self.tree.read().await;
            (tree.len(), tree.root().map_err(|_| IndexError::Tree)?)
        };
        // The pool records no root until something is in it.
        if leaves > 0 && !self.source.is_known_root(root).await? {
            return Err(IndexError::Diverged { leaves });
        }
        Ok(Health {
            leaves,
            block,
            root,
        })
    }
}
