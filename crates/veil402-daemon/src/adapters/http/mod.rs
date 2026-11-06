//! The agent API: `OpenAI`'s shape over a shielded note.

mod primitives;
mod routes;

pub use primitives::Refusal;
pub use routes::{Api, router};
