//! Deterministic, I/O-free gateway logic.

mod ports;
mod service;

pub use ports::{
    Accepted, Billed, Escrow, Escrows, Kept, Metering, Meters, Opened, Replay, Replayed,
    Resumes, Served,
};
pub use service::{MeterError, MeteringService, Serving};
