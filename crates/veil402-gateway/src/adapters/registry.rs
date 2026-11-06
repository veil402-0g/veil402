//! The serving contract, which is where a provider's attested key belongs.
//!
//! Pinning the endpoint and the signer by hand meant trusting whoever wrote
//! the configuration. The chain already holds both, acknowledged by the
//! provider, so an auditor can read the same record we checked against.

use std::collections::HashMap;

use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder},
    sol,
};
use async_trait::async_trait;
use tokio::sync::RwLock;
use veil402_enclave::{Attestor, Registry};

sol! {
    #[sol(rpc)]
    interface IInferenceServing {
        struct Service {
            address provider;
            string serviceType;
            string url;
            uint256 inputPrice;
            uint256 outputPrice;
            uint256 updatedAt;
            string model;
            string verifiability;
            string additionalInfo;
            address teeSignerAddress;
            bool teeSignerAcknowledged;
        }
        function getService(address provider) external view returns (Service memory);
    }
}

pub struct Registrar<P> {
    inner: P,
    contract: Address,
    /// Read once per provider. The record changes when a provider
    /// re-registers, which is rare, and a stale endpoint fails the
    /// signature check rather than passing a wrong one.
    known: RwLock<HashMap<Address, Option<Attestor>>>,
}

/// # Errors
///
/// Returns the transport error when the endpoint cannot be reached.
pub async fn registrar(
    rpc: &str,
    contract: Address,
) -> Result<Registrar<impl Provider + 'static>, String> {
    Ok(Registrar {
        inner: ProviderBuilder::new()
            .connect(rpc)
            .await
            .map_err(|error| error.to_string())?,
        contract,
        known: RwLock::new(HashMap::new()),
    })
}

impl<P: Provider> Registrar<P> {
    async fn fetch(&self, provider: Address) -> Option<Attestor> {
        let service = IInferenceServing::new(self.contract, &self.inner)
            .getService(provider)
            .call()
            .await
            .ok()?;
        // A key the provider has not acknowledged is one it has not agreed
        // to be held to.
        (service.teeSignerAcknowledged && !service.url.is_empty()).then_some(Attestor {
            endpoint: service.url,
            signer: service.teeSignerAddress,
        })
    }
}

#[async_trait]
impl<P: Provider + 'static> Registry for Registrar<P> {
    async fn attestor(&self, provider: Address) -> Option<Attestor> {
        if let Some(known) = self.known.read().await.get(&provider) {
            return known.clone();
        }
        let fetched = self.fetch(provider).await;
        self.known.write().await.insert(provider, fetched.clone());
        fetched
    }
}
