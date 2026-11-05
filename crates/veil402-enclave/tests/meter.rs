use alloy::{
    primitives::{B256, Signature},
    signers::local::PrivateKeySigner,
};
use veil402_enclave::{Call, Error, Meter, sign_lock};
use veil402_sdk::{
    Field,
    evm::{PoolClient, Session, Voucher},
};

fn pool() -> PoolClient {
    PoolClient::new(
        "0x42532488f967329e2B090dCf68396Ef0dd63de69".parse().expect("pool"),
        "0x0000000000000000000000000000000000000abc".parse().expect("token"),
        "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC".parse().expect("verifier"),
        296,
    )
    .expect("client")
}

fn session() -> Session {
    Session {
        nullifier: Field::from(7_u64),
        seller: "0x000000000000000000000000000000000000dEaD".parse().expect("seller"),
        deadline: 1_900_000_000,
    }
}

fn call(cost: u64) -> Call {
    Call {
        cost,
        attestation: B256::repeat_byte(u8::try_from(cost % 256).unwrap_or(0)),
    }
}

#[test]
fn totals_accumulate_and_stop_at_the_ceiling() {
    let mut meter = Meter::new(pool(), session(), 100);
    assert_eq!(meter.record(call(30)).expect("first"), 30);
    assert_eq!(meter.record(call(70)).expect("second"), 100);
    assert_eq!(meter.calls(), 2);

    // Serving past the ceiling would produce a settlement nobody can prove.
    assert!(matches!(meter.record(call(1)), Err(Error::CeilingReached)));
    assert_eq!(meter.total(), 100);
    assert_eq!(meter.calls(), 2);
}

#[test]
fn vouchers_are_cumulative_so_only_the_newest_matters() {
    let signer = PrivateKeySigner::random();
    let mut meter = Meter::new(pool(), session(), 1_000);

    assert!(matches!(meter.voucher(&signer), Err(Error::NothingOwed)));

    meter.record(call(30)).expect("first");
    let first = meter.voucher(&signer).expect("voucher");
    meter.record(call(45)).expect("second");
    let second = meter.voucher(&signer).expect("voucher");

    assert_eq!(first.amount, 30);
    assert_eq!(second.amount, 75, "a voucher carries the running total");
    assert_ne!(first.calls_root, second.calls_root, "the root covers every call");
    assert_eq!(second.signature.len(), 65);
}

#[test]
fn signatures_recover_to_the_enclave() -> Result<(), Box<dyn std::error::Error>> {
    let signer = PrivateKeySigner::random();
    let client = pool();
    let session = session();

    let lock = sign_lock(&client, &session, &signer)?;
    assert_eq!(
        Signature::try_from(lock.as_slice())?
            .recover_address_from_prehash(&client.lock_digest(&session))?,
        signer.address()
    );

    let mut meter = Meter::new(client, session, 1_000);
    meter.record(call(30))?;
    let voucher: Voucher = meter.voucher(&signer)?;
    let digest = pool().settlement_digest(&voucher.session, voucher.amount, voucher.calls_root);
    assert_eq!(
        Signature::try_from(voucher.signature.as_slice())?.recover_address_from_prehash(&digest)?,
        signer.address()
    );
    Ok(())
}
