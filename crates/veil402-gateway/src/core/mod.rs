//! Deterministic, I/O-free gateway logic.

mod ports;
mod service;

pub use ports::{Kept, Metering, Meters, Prices, Replayed, Resumes, Served};
pub use service::{MeterError, MeteringService, Serving};
