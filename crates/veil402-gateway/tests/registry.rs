//! The serving contract, against the real chain.
//!
//! Pins three things nothing else catches until a call fails quietly: the
//! mainnet address, the `Service` layout we decode, and that the provider
//! we buy from still acknowledges the key we check signatures against.
//! Opt-in, because it reaches the network.

use alloy::primitives::address;
use veil402_enclave::Registry;
use veil402_gateway::adapters::registrar;

type Fallible = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore = "reads 0G mainnet"]
async fn the_serving_contract_names_the_key_we_verify_against() -> Fallible {
    let registry = registrar(
        "https://evmrpc.0g.ai",
        address!("47340d900bdFec2BD393c626E12ea0656F938d84"),
    )
    .await?;

    let attestor = registry
        .attestor(address!("7DCFe6AEa70350C2090041524c9B4A9262DCe87D"))
        .await
        .ok_or("the provider is unregistered, or has not acknowledged its key")?;

    assert_eq!(
        attestor.signer,
        address!("34101E02F34a8720172249Ec06630BEC32dE30E7"),
        "the attested signer changed"
    );
    assert!(
        attestor.endpoint.starts_with("https://"),
        "{}",
        attestor.endpoint
    );

    // An unregistered address answers nothing rather than guessing.
    let absent = registry
        .attestor(address!("0000000000000000000000000000000000000001"))
        .await;
    assert!(absent.is_none(), "an unregistered provider was vouched for");
    Ok(())
}
