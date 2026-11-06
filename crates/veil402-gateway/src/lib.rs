//! The untrusted half of the seller side.
//!
//! Terminates the buyer-facing protocol, holds what the enclave cannot — the
//! prices, and which calls were already answered — and relays the rest. It
//! sees ciphertext and totals; it never sees a prompt or a key.
//!
//! `core` is I/O-free and declares its ports, `adapters` implements them, and
//! `app` wires them, which is how the daemon is built and why the move to
//! vsock replaces one file.

pub mod adapters;
pub mod app;
pub mod core;
