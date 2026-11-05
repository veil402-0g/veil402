//! EVM transport: the values a Veil pool binds on chain, and the calldata that
//! carries them.

use alloy::sol;

mod asset;
mod binding;
mod calldata;
mod calls;
mod client;
mod escrow;

// Generated from the compiled contract, so a signature cannot drift from it.
sol!(
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    Pool,
    "abi/pool.json",
);

pub(crate) use binding::External;
pub(crate) use client::transact;

pub use Pool::ExtData;
pub use asset::asset_id;
pub use binding::{Withdrawal, ext_data_hash, public_amount};
pub use calldata::ProofCalldata;
pub use calls::{Call, calls_proof, calls_root};
pub use escrow::{Session, Voucher};
pub use client::PoolClient;
