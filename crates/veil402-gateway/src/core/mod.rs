//! Deterministic, I/O-free gateway logic.

mod ports;
mod service;

pub use ports::{Meters, Prices, Served};
pub use service::{MeterError, MeteringService, Serving};
