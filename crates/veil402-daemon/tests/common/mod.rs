//! Fakes shared by the index and sync suites: a pool that can misbehave in
//! each of the ways the daemon has to tell apart, and a store that mirrors
//! what the real one does with nodes.

#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use async_trait::async_trait;
use veil402_daemon::core::{
    StoreError,
    index::{Batch, ChainError, Commitment, CommitmentSource, IndexStore, Nullified},
};
use veil402_sdk::{Field, StoredNode, Tree};

pub type Fallible = Result<(), Box<dyn std::error::Error>>;

#[must_use]
pub fn field(value: u64) -> Field {
    Field::from(value)
}

/// How the endpoint behaves: present, away entirely, or dropping three passes
/// out of four.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum Relay {
    #[default]
    Up,
    Down,
    Flaky,
}

/// A pool holding `held` commitments, one per block. `withheld` is a leaf the
/// source refuses to report while the root still counts it, which is what a
/// missed event looks like.
pub struct Chain {
    held: Vec<Field>,
    spent: Vec<Field>,
    withheld: Option<usize>,
    corrupted: Option<usize>,
    relay: Relay,
    calls: AtomicU32,
}

impl Chain {
    #[must_use]
    pub fn new(count: u64) -> Self {
        Self {
            held: (1..=count).map(field).collect(),
            spent: Vec::new(),
            withheld: None,
            corrupted: None,
            relay: Relay::Up,
            calls: AtomicU32::new(0),
        }
    }

    /// Skips a leaf entirely, which is what a dropped log looks like.
    #[must_use]
    pub fn withholding(count: u64, leaf: usize) -> Self {
        Self {
            withheld: Some(leaf),
            ..Self::new(count)
        }
    }

    /// Reports every leaf but one with the wrong value, so only the root
    /// notices.
    #[must_use]
    pub fn corrupting(count: u64, leaf: usize) -> Self {
        Self {
            corrupted: Some(leaf),
            ..Self::new(count)
        }
    }

    /// A pool that has also recorded nullifiers.
    #[must_use]
    pub fn spending(count: u64, spent: &[u64]) -> Self {
        Self {
            spent: spent.iter().copied().map(field).collect(),
            ..Self::new(count)
        }
    }

    #[must_use]
    pub fn down(count: u64) -> Self {
        Self {
            relay: Relay::Down,
            ..Self::new(count)
        }
    }

    #[must_use]
    pub fn flaky(count: u64) -> Self {
        Self {
            relay: Relay::Flaky,
            ..Self::new(count)
        }
    }

    fn refuses(&self) -> bool {
        match self.relay {
            Relay::Up => false,
            Relay::Down => true,
            Relay::Flaky => self.calls.fetch_add(1, Ordering::Relaxed) % 4 != 3,
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
        if self.refuses() {
            return Err(ChainError::Unavailable);
        }
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
            locks: Vec::new(),
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
pub struct Memory {
    commitments: Mutex<Vec<Commitment>>,
    nodes: Mutex<BTreeMap<(u8, u32), Field>>,
    cursor: Mutex<u64>,
    spent: Mutex<HashSet<Field>>,
    refusing: AtomicBool,
}

impl Memory {
    pub fn refuse(&self) {
        self.refusing.store(true, Ordering::Relaxed);
    }

    fn held<T>(lock: &Mutex<T>) -> Result<MutexGuard<'_, T>, StoreError> {
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
