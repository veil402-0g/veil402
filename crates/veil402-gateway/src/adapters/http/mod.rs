//! The HTTP surface. One of several possible adapters over the same core.

mod primitives;
mod routes;

pub use primitives::{Response, Status};
pub use routes::router;
