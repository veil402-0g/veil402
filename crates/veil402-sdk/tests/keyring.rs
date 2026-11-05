//! Every enclave key comes from one root seed, so a restart re-derives them.

use alloy::signers::{SignerSync, local::PrivateKeySigner};
use veil402_sdk::{
    Field,
    evm::PoolClient,
    session::{Keyring, Recipient, RootSeed},
};

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn pool() -> Result<PoolClient, veil402_sdk::Error> {
    PoolClient::new(
        "0x42532488f967329e2B090dCf68396Ef0dd63de69".parse().expect("pool"),
        "0x0000000000000000000000000000000000000abc".parse().expect("token"),
        "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC".parse().expect("verifier"),
        296,
    )
}

fn keyring(byte: u8) -> Keyring {
    Keyring::new(RootSeed::from_bytes([byte; 32]))
}

/// Restart reaches the same keys, and nothing else does.
///
/// Both halves matter: a key that moved would strand every envelope in flight
/// and every escrow the registry's address agreed to meter, and one that did
/// not separate per envelope would let a single leak open all of them.
#[test]
fn a_root_seed_fixes_every_key_it_derives() -> Fallible {
    let (first, second) = (Field::from(7_u64), Field::from(8_u64));

    assert_eq!(keyring(1).handover(first).public(), keyring(1).handover(first).public());
    assert_eq!(keyring(1).signer()?.address(), keyring(1).signer()?.address());
    assert_eq!(keyring(1).commitment(), keyring(1).commitment());

    assert_ne!(keyring(1).handover(first).public(), keyring(1).handover(second).public());
    assert_ne!(keyring(1).handover(first).public(), keyring(2).handover(first).public());
    assert_ne!(keyring(1).signer()?.address(), keyring(2).signer()?.address());
    assert_ne!(keyring(1).commitment(), keyring(2).commitment());
    Ok(())
}

/// The channel still works end to end on a derived key, and only for the
/// envelope it was derived for.
#[test]
fn a_derived_key_opens_only_its_own_envelope() -> Fallible {
    let client = pool()?;
    let key = keyring(1).handover(Field::from(7_u64));
    let vouching = PrivateKeySigner::random();
    let signature = vouching.sign_hash_sync(&key.announcement(&client))?.as_bytes();

    let sealed = Recipient::accept(key.public(), &client, vouching.address(), &signature)?
        .seal(b"delegation")?;

    assert_eq!(key.open(&sealed)?, b"delegation");
    assert!(
        keyring(1).handover(Field::from(8_u64)).open(&sealed).is_err(),
        "another envelope's key opened it"
    );
    Ok(())
}
