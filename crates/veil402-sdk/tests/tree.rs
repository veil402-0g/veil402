use std::{collections::BTreeMap, error::Error as StdError};

use ark_bn254::Fr;
use light_poseidon::{Poseidon, PoseidonBytesHasher};
use veil402_sdk::{Field, MerklePath, StoredNode, Tree};

fn field(value: u64) -> Field {
    Field::from(value)
}

/// Mirrors what a store does with the pending writes: later writes to a
/// position replace earlier ones, and the result comes back in order.
#[derive(Default)]
struct Persisted(BTreeMap<(u8, u32), Field>);

impl Persisted {
    fn take(&mut self, tree: &mut Tree) -> Vec<StoredNode> {
        for held in tree.pending() {
            self.0.insert((held.level, held.index), held.node);
        }
        tree.commit();
        self.0
            .iter()
            .map(|((level, index), node)| StoredNode {
                level: *level,
                index: *index,
                node: *node,
            })
            .collect()
    }
}

/// The pool derives its root from the same zero chain, so an empty tree has to
/// agree before anything else can.
#[test]
fn empty_root_is_the_top_of_the_zero_chain() -> Result<(), Box<dyn StdError>> {
    let tree = Tree::new();
    assert!(tree.is_empty());
    assert_eq!(
        format!("0x{}", hex::encode(tree.root()?.to_bytes())),
        "0x2134e76ac5d21aab186c2be1dd8f84ee880a1e46eaf712f9d371b6df22191f3e"
    );
    Ok(())
}

/// The first leaf's path is every empty subtree, which is the case
/// `MerklePath::only_leaf` covers without a tree at all.
#[test]
fn first_leaf_matches_the_standalone_path() -> Result<(), Box<dyn StdError>> {
    let mut tree = Tree::new();
    assert_eq!(tree.insert(field(42))?, 0);

    let witness = tree.witness(&[0])?;
    let lone = MerklePath::only_leaf()?;
    assert_eq!(witness.paths[0].index, lone.index);
    assert_eq!(witness.paths[0].siblings, lone.siblings);
    Ok(())
}

/// Every leaf must authenticate against the one root the witness carries, which
/// is what lets two notes be spent in the same proof.
#[test]
fn every_leaf_authenticates_against_one_root() -> Result<(), Box<dyn StdError>> {
    let mut tree = Tree::new();
    let leaves: Vec<Field> = (1..=9_u64).map(field).collect();
    for leaf in &leaves {
        tree.insert(*leaf)?;
    }
    assert_eq!(tree.len(), 9);

    let indices: Vec<u32> = (0..9).collect();
    let witness = tree.witness(&indices)?;
    assert_eq!(witness.root, tree.root()?);
    for (index, (leaf, path)) in leaves.iter().zip(&witness.paths).enumerate() {
        assert_eq!(
            recompute(*leaf, path),
            witness.root,
            "leaf {index} does not authenticate"
        );
    }
    Ok(())
}

#[test]
fn rejects_a_leaf_it_does_not_hold() -> Result<(), Box<dyn StdError>> {
    let mut tree = Tree::new();
    tree.insert(field(1))?;
    assert!(tree.witness(&[1]).is_err(), "witness for a missing leaf");
    Ok(())
}

/// Restoring from stored nodes is the whole point of persisting them: it has to
/// land on the same tree without hashing the history again.
#[test]
fn stored_nodes_restore_the_same_tree() -> Result<(), Box<dyn StdError>> {
    let mut tree = Tree::new();
    let mut persisted = Persisted::default();
    let mut nodes = Vec::new();
    for leaf in 1..=7_u64 {
        tree.insert(field(leaf))?;
        nodes = persisted.take(&mut tree);
    }

    let restored = Tree::load(&nodes)?;
    assert_eq!(restored.len(), tree.len());
    assert_eq!(restored.root()?, tree.root()?);
    assert_eq!(restored.witness(&[0, 6])?.paths[1].siblings, tree.witness(&[0, 6])?.paths[1].siblings);
    Ok(())
}

/// A store that refuses a write must leave the tree exactly where the store is,
/// or the next path it hands out authenticates against a root nobody holds.
#[test]
fn rollback_returns_the_tree_to_its_last_commit() -> Result<(), Box<dyn StdError>> {
    let mut tree = Tree::new();
    for leaf in 1..=3_u64 {
        tree.insert(field(leaf))?;
    }
    tree.commit();
    let root = tree.root()?;
    let path = tree.witness(&[2])?.paths.remove(0);

    for leaf in 4..=6_u64 {
        tree.insert(field(leaf))?;
    }
    assert_ne!(tree.root()?, root, "the tree did move");
    tree.rollback();

    assert_eq!(tree.len(), 3);
    assert_eq!(tree.root()?, root);
    assert_eq!(tree.witness(&[2])?.paths[0].siblings, path.siblings);
    assert!(tree.pending().is_empty());
    Ok(())
}

/// Mirrors the circuit's `compute_root`: a set bit means the node is the right
/// child.
fn recompute(leaf: Field, path: &MerklePath) -> Field {
    let mut node = leaf;
    for (level, sibling) in path.siblings.iter().enumerate() {
        let (left, right) = if path.index & (1 << level) == 0 {
            (node, *sibling)
        } else {
            (*sibling, node)
        };
        node = hash(left, right);
    }
    node
}

fn hash(left: Field, right: Field) -> Field {
    let mut hasher = Poseidon::<Fr>::new_circom(2).expect("poseidon");
    let bytes = hasher
        .hash_bytes_be(&[&left.to_bytes(), &right.to_bytes()])
        .expect("hash");
    Field::from_bytes(bytes).expect("field")
}
