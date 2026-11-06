mod disclosure;
pub(crate) mod encoding;
mod field;
pub(crate) mod note;
mod transaction;
mod tree;

pub use disclosure::Disclosure;
pub use field::{Field, Secret};
pub use note::{Note, Owner, Send, SessionKeys};
pub use transaction::{MerklePath, PublicInputs, Spend, Transaction};
pub use tree::{StoredNode, Tree, Witness};
pub(crate) use transaction::{Plan, Public, Values};

/// Number of siblings in a transaction circuit Merkle path.
pub const TREE_DEPTH: usize = 20;

/// Largest ciphertext the pool accepts for one output note.
pub const MAX_NOTE_BYTES: usize = 128;
