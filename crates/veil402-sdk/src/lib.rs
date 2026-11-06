//! Canonical client library for constructing and proving Veil transactions.

mod client;
pub mod evm;
mod protocol;
pub mod session;
pub mod wire;
mod proving;

pub use client::{Config, Error, Prepared, Proof, Veil};
pub use protocol::{
    Field, MAX_NOTE_BYTES, MerklePath, Note, Owner, PublicInputs, Secret, Send, SessionKeys, Spend,
    StoredNode, TREE_DEPTH, Transaction, Tree, Witness,
};
