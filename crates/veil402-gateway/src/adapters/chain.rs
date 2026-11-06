use alloy::{
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    sol,
};
use async_trait::async_trait;
use veil402_sdk::Field;

use crate::core::{Escrow, Escrows, MeterError};

sol! {
    #[sol(rpc)]
    interface IPool {
        function escrows(uint256 nullifier)
            external view returns (address enclave, address seller, uint64 deadline, bool settled);
        function knownRoot(uint256 root) external view returns (bool);
    }
}

fn unreachable(error: impl std::fmt::Display) -> MeterError {
    MeterError::Refused(format!("the chain could not be read: {error}"))
}

/// Reads the pool, so the host can check what the enclave cannot.
///
/// Read-only: this side holds no key that moves anything, and the only
/// questions it asks are ones a block explorer would answer.
pub struct OnChain<P> {
    provider: P,
    pool: Address,
}

/// # Errors
///
/// Returns [`MeterError::Refused`] when the endpoint cannot be reached.
pub async fn connect(
    rpc: &str,
    pool: Address,
) -> Result<OnChain<impl Provider + 'static>, MeterError> {
    Ok(OnChain {
        provider: ProviderBuilder::new()
            .connect(rpc)
            .await
            .map_err(unreachable)?,
        pool,
    })
}

#[async_trait]
impl<P: Provider + Send + Sync> Escrows for OnChain<P> {
    async fn escrow(&self, nullifier: Field) -> Result<Option<Escrow>, MeterError> {
        let held = IPool::new(self.pool, &self.provider)
            .escrows(U256::from_be_bytes(nullifier.to_bytes()))
            .call()
            .await
            .map_err(unreachable)?;
        // The pool leaves an unreserved nullifier zeroed, so a zero enclave is
        // the absence of an escrow rather than a strange one.
        if held.enclave.is_zero() {
            return Ok(None);
        }
        Ok(Some(Escrow {
            enclave: held.enclave,
            seller: held.seller,
            deadline: held.deadline,
            settled: held.settled,
        }))
    }

    async fn is_known_root(&self, root: Field) -> Result<bool, MeterError> {
        IPool::new(self.pool, &self.provider)
            .knownRoot(U256::from_be_bytes(root.to_bytes()))
            .call()
            .await
            .map_err(unreachable)
    }
}
