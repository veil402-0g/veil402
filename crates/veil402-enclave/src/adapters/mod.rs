//! Where the enclave reaches the world outside it.

mod router;

pub use router::Router;

pub mod nums;

// Only inside an enclave. The security module and vsock do not exist
// elsewhere, and the modules that use them would not build on a developer's
// machine — so the in-process meter stays usable without this half.
#[cfg(target_os = "linux")]
pub mod kms;
#[cfg(target_os = "linux")]
pub mod nsm;
#[cfg(target_os = "linux")]
pub mod relay;
#[cfg(target_os = "linux")]
pub mod seed;
