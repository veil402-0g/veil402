use std::{
    cmp::Ordering,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use rs_merkle_tree::{MerkleError, Node, Store, hasher::PoseidonHasher, tree::MerkleTree as Inner};

use super::{Field, MerklePath, TREE_DEPTH};
use crate::client::Error;

/// Node levels, counting both the leaves and the root.
const LEVELS: usize = TREE_DEPTH + 1;

/// A tree node at the position it is stored under.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredNode {
    /// Distance from the leaves.
    pub level: u8,
    /// Position within the level.
    pub index: u32,
    /// Node value.
    pub node: Field,
}

/// Membership data for a set of leaves against a single root.
///
/// The circuit takes one root, so paths drawn from different tree states cannot
/// be proved together.
#[derive(Debug)]
pub struct Witness {
    /// Root every path authenticates against.
    pub root: Field,
    /// Paths in the order the leaves were asked for.
    pub paths: Vec<MerklePath>,
}

fn node(field: Field) -> Node {
    Node::from(field.to_bytes())
}

fn field(node: Node) -> Result<Field, Error> {
    let bytes: [u8; 32] = node.as_ref().try_into().map_err(|_| Error::Field)?;
    Field::from_bytes(bytes)
}

fn misplaced() -> MerkleError {
    MerkleError::StoreError("node position is outside the tree".into())
}

fn position(level: u32, index: u64) -> Option<(usize, usize)> {
    let level = usize::try_from(level).ok().filter(|held| *held < LEVELS)?;
    let index = usize::try_from(index)
        .ok()
        .filter(|held| *held < 1 << TREE_DEPTH)?;
    Some((level, index))
}

/// Dense per-level node storage.
///
/// Every level of an append-only tree fills contiguously from zero, so a level
/// is a vector and a lookup is an index. Writes are also recorded, because a
/// tree that is never rebuilt has to be persisted as it grows.
#[derive(Default)]
struct Dense {
    levels: [Vec<Node>; LEVELS],
    marks: [usize; LEVELS],
    frontier: [Option<Node>; LEVELS],
    pending: Vec<StoredNode>,
}

impl Dense {
    fn load(nodes: &[StoredNode]) -> Result<Self, Error> {
        let mut dense = Self::default();
        for held in nodes {
            dense
                .write(
                    u32::from(held.level),
                    u64::from(held.index),
                    node(held.node),
                )
                .map_err(|_| Error::Transaction("stored nodes are out of order"))?;
        }
        dense.commit();
        Ok(dense)
    }

    fn at(&self, level: u32, index: u64) -> Option<Node> {
        let (level, index) = position(level, index)?;
        self.levels.get(level)?.get(index).copied()
    }

    fn write(&mut self, level: u32, index: u64, value: Node) -> Result<(), MerkleError> {
        let (level, index) = position(level, index).ok_or_else(misplaced)?;
        let holding = self.levels.get_mut(level).ok_or_else(misplaced)?;
        match index.cmp(&holding.len()) {
            Ordering::Less => holding[index] = value,
            Ordering::Equal => holding.push(value),
            Ordering::Greater => return Err(misplaced()),
        }
        self.pending.push(StoredNode {
            level: u8::try_from(level).map_err(|_| misplaced())?,
            index: u32::try_from(index).map_err(|_| misplaced())?,
            node: field(value).map_err(|_| misplaced())?,
        });
        Ok(())
    }

    fn commit(&mut self) {
        for (level, holding) in self.levels.iter().enumerate() {
            self.marks[level] = holding.len();
            self.frontier[level] = holding.last().copied();
        }
        self.pending.clear();
    }

    /// Only the rightmost node of a level is ever overwritten, so restoring it
    /// alongside the truncation returns the level exactly to its last commit.
    fn rollback(&mut self) {
        for (level, holding) in self.levels.iter_mut().enumerate() {
            holding.truncate(self.marks[level]);
            if let (Some(node), Some(slot)) = (self.frontier[level], holding.last_mut()) {
                *slot = node;
            }
        }
        self.pending.clear();
    }
}

/// The tree owns its store but the caller has to persist what it writes, so the
/// two hold it jointly.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Dense>>);

impl Shared {
    fn held(&self) -> MutexGuard<'_, Dense> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Store for Shared {
    fn get(&self, levels: &[u32], indices: &[u64]) -> Result<Vec<Option<Node>>, MerkleError> {
        if levels.len() != indices.len() {
            return Err(MerkleError::LengthMismatch {
                levels: levels.len(),
                indices: indices.len(),
            });
        }
        let held = self.held();
        Ok(levels
            .iter()
            .zip(indices)
            .map(|(level, index)| held.at(*level, *index))
            .collect())
    }

    fn put(&mut self, items: &[(u32, u64, Node)]) -> Result<(), MerkleError> {
        let mut held = self.held();
        items
            .iter()
            .try_for_each(|(level, index, value)| held.write(*level, *index, *value))
    }

    fn get_num_leaves(&self) -> u64 {
        u64::try_from(self.held().levels[0].len()).unwrap_or(u64::MAX)
    }
}

/// The pool's commitment tree, mirrored from the commitments it emitted.
///
/// The pool keeps only the roots it has reached; the siblings a spend needs
/// exist nowhere on chain, so anything that wants to spend has to carry the
/// tree itself.
pub struct Tree {
    inner: Inner<PoseidonHasher, Shared, TREE_DEPTH>,
    levels: Shared,
}

impl Tree {
    /// Builds an empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self::over(Shared::default())
    }

    /// Restores a tree from its stored nodes, which costs no hashing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transaction`] when the nodes do not arrive in level and
    /// index order, which would leave a level with a hole in it.
    pub fn load(nodes: &[StoredNode]) -> Result<Self, Error> {
        Ok(Self::over(Shared(Arc::new(Mutex::new(Dense::load(
            nodes,
        )?)))))
    }

    fn over(levels: Shared) -> Self {
        Self {
            inner: Inner::new(PoseidonHasher, levels.clone()),
            levels,
        }
    }

    /// Appends a commitment and returns the leaf index it landed at.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transaction`] when the tree is full.
    pub fn insert(&mut self, commitment: Field) -> Result<u32, Error> {
        let index = self.len();
        self.inner
            .add_leaves(&[node(commitment)])
            .map_err(|_| Error::Transaction("tree is full"))?;
        Ok(index)
    }

    /// Nodes written since the last commit, for the caller to persist.
    #[must_use]
    pub fn pending(&self) -> Vec<StoredNode> {
        self.levels.held().pending.clone()
    }

    /// Accepts the pending writes, once they are durable.
    pub fn commit(&mut self) {
        self.levels.held().commit();
    }

    /// Drops the pending writes, which keeps the tree from running ahead of the
    /// store when a write fails.
    pub fn rollback(&mut self) {
        self.levels.held().rollback();
    }

    /// Returns the current root, which must equal the pool's.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Field`] when the tree holds a non-canonical node.
    pub fn root(&self) -> Result<Field, Error> {
        field(self.inner.root().map_err(|_| Error::Field)?)
    }

    /// Returns how many commitments the tree holds.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.inner.num_leaves()).unwrap_or(u32::MAX)
    }

    /// Returns whether no commitment has been inserted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns paths for `leaves`, all against the current root.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transaction`] when the tree holds no such leaf.
    pub fn witness(&self, leaves: &[u32]) -> Result<Witness, Error> {
        Ok(Witness {
            root: self.root()?,
            paths: leaves
                .iter()
                .map(|leaf| self.path(*leaf))
                .collect::<Result<_, _>>()?,
        })
    }

    /// The tree is happy to prove an empty leaf at any index; proving a note it
    /// does not hold only wastes a proof, so it is refused here.
    fn path(&self, index: u32) -> Result<MerklePath, Error> {
        if index >= self.len() {
            return Err(Error::Transaction("leaf is not in the tree"));
        }
        let proof = self
            .inner
            .proof(u64::from(index))
            .map_err(|_| Error::Transaction("leaf is not in the tree"))?;

        let mut siblings = [Field::ZERO; TREE_DEPTH];
        for (sibling, held) in siblings.iter_mut().zip(proof.proof) {
            *sibling = field(held)?;
        }
        Ok(MerklePath { index, siblings })
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}
