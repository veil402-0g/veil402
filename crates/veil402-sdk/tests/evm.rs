use std::{error::Error as StdError, fs, path::Path};

use alloy::primitives::{Address, Bytes, I256, U256};
use veil402_sdk::{
    Field, MAX_NOTE_BYTES,
    evm::{
        ExtData, PoolClient, ProofCalldata, Session, Withdrawal, asset_id, ext_data_hash,
        public_amount,
    },
};

const POOL: &str = "0x42532488f967329e2B090dCf68396Ef0dd63de69";
const TOKEN: &str = "0x0000000000000000000000000000000000000abc";
const VERIFIER: &str = "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC";
const CHAIN_ID: u64 = 296;

fn client() -> PoolClient {
    PoolClient::new(address(POOL), address(TOKEN), address(VERIFIER), CHAIN_ID).expect("client")
}

fn address(value: &str) -> Address {
    value.parse().expect("address literal")
}

fn ext(recipient: &str, amount: i64, notes: [&[u8]; 2]) -> ExtData {
    ExtData {
        recipient: address(recipient),
        extAmount: I256::unchecked_from(amount),
        notes: [Bytes::copy_from_slice(notes[0]), Bytes::copy_from_slice(notes[1])],
    }
}

fn root() -> Result<&'static Path, Box<dyn StdError>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "workspace root is unavailable".into())
}

fn hex(field: Field) -> String {
    format!("0x{}", hex::encode(field.to_bytes()))
}

/// The cases the Solidity side has to reproduce. Lengths differ between the two
/// notes so a missing length prefix would let bytes shift across the boundary.
fn cases() -> Vec<(&'static str, i64, [Vec<u8>; 2])> {
    vec![
        ("0x000000000000000000000000000000000000dEaD", 0, [vec![], vec![]]),
        ("0x000000000000000000000000000000000000dEaD", -300, [vec![0xaa, 0xbb], vec![0xcc, 0xdd]]),
        ("0x0000000000000000000000000000000000000000", 0, [vec![0xaa], vec![0xbb, 0xcc, 0xdd]]),
        ("0x000000000000000000000000000000000000dEaD", -1, [vec![0xaa, 0xbb, 0xcc], vec![0xdd]]),
        (
            "0x73Cd11793191573237cb7a6DedCCa7cC9FDBD6ed",
            -9_223_372_036_854_775_807,
            [vec![0x11; MAX_NOTE_BYTES], vec![0x22; MAX_NOTE_BYTES]],
        ),
    ]
}

#[test]
fn public_amount_wraps_withdrawals_into_the_field() {
    assert_eq!(public_amount(0), Field::ZERO);
    // r - 1000, the additive inverse the circuit reads.
    assert_eq!(
        hex(public_amount(-1000)),
        "0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593effffc19"
    );
}

#[test]
fn rejects_data_the_pool_would_reject() {
    let pool = client();
    assert!(ext_data_hash(&pool, &ext(POOL, 1, [&[], &[]])).is_err(), "deposit accepted");
    assert!(
        ext_data_hash(&pool, &ext(POOL, 0, [&[0; MAX_NOTE_BYTES + 1], &[]])).is_err(),
        "oversize note accepted"
    );
}

#[test]
fn withdrawal_rejects_what_the_pool_would() {
    let to = address("0x000000000000000000000000000000000000dEaD");
    assert!(Withdrawal::new(to, 0).is_err(), "zero amount accepted");
    assert!(Withdrawal::new(Address::ZERO, 1).is_err(), "burn accepted");
    let too_wide = u64::try_from(i64::MAX).expect("i64::MAX fits u64") + 1;
    assert!(Withdrawal::new(to, too_wide).is_err(), "unrepresentable amount accepted");

    let ok = Withdrawal::new(to, 300).expect("valid withdrawal");
    assert_eq!(ok.amount(), 300);
    assert_eq!(ok.recipient(), to);
}

#[test]
fn binding_is_specific_to_the_deployment() -> Result<(), Box<dyn StdError>> {
    let data = ext("0x000000000000000000000000000000000000dEaD", -300, [&[0xaa], &[0xbb]]);
    let other = address("0x0000000000000000000000000000000000000001");
    let here = ext_data_hash(&client(), &data)?;

    for (label, pool) in [
        ("chain id", PoolClient::new(address(POOL), address(TOKEN), address(VERIFIER), 1)?),
        ("pool", PoolClient::new(other, address(TOKEN), address(VERIFIER), CHAIN_ID)?),
        ("token", PoolClient::new(address(POOL), other, address(VERIFIER), CHAIN_ID)?),
        ("verifier", PoolClient::new(address(POOL), address(TOKEN), other, CHAIN_ID)?),
    ] {
        assert_ne!(here, ext_data_hash(&pool, &data)?, "{label} not bound");
    }
    Ok(())
}

/// The Go tool and this parser read the same raw proof; they must agree on
/// every coordinate, including the EIP-197 ordering of the G2 point.
///
/// Reads a frozen pair rather than the live proof fixture, which the proving
/// test rewrites with fresh randomness.
#[test]
fn calldata_matches_the_go_encoder() -> Result<(), Box<dyn StdError>> {
    let expected: serde_json::Value = serde_json::from_slice(&fs::read(
        root()?.join("contracts/test/fixtures/calldata-vector.json"),
    )?)?;

    let raw = hex::decode(
        expected["raw"]
            .as_str()
            .ok_or("fixture is missing its proof")?
            .trim_start_matches("0x"),
    )?;
    let parsed = ProofCalldata::parse(&raw)?;

    let compare = |key: &str, values: &[U256]| -> Result<(), Box<dyn StdError>> {
        for (index, value) in values.iter().enumerate() {
            let want = expected[key][index].as_str().ok_or("fixture entry is missing")?;
            assert_eq!(
                format!("0x{value:x}"),
                want,
                "{key}[{index}] disagrees with the Go encoder"
            );
        }
        Ok(())
    };
    compare("proof", &parsed.proof)?;
    compare("commitments", &parsed.commitments)?;
    compare("commitmentPok", &parsed.commitment_pok)?;
    Ok(())
}

#[test]
fn rejects_malformed_proofs() {
    assert!(ProofCalldata::parse(&[0; 387]).is_err(), "short proof accepted");
    // All-zero coordinates are the point at infinity, which gnark never emits.
    let mut raw = [0_u8; 388];
    raw[259] = 1;
    raw[0] = 0xff;
    assert!(ProofCalldata::parse(&raw).is_err(), "off-curve point accepted");
}

/// Writes the vectors the Solidity test asserts against. Parallel arrays keep
/// the forge-side JSON path simple.
#[test]
fn writes_binding_fixture() -> Result<(), Box<dyn StdError>> {
    let pool = client();
    let tokens = [
        TOKEN,
        "0x0000000000000000000000000000000000000001",
        "0xffffffffffffffffffffffffffffffffffffffff",
        "0x73Cd11793191573237cb7a6DedCCa7cC9FDBD6ed",
    ];
    let asset_ids: Vec<String> = tokens
        .iter()
        .map(|token| Ok(hex(asset_id(address(token))?)))
        .collect::<Result<_, Box<dyn StdError>>>()?;

    assert_eq!(hex(pool.asset()), asset_ids[0], "client disagrees with asset_id");

    let mut recipients = Vec::new();
    let mut amounts = Vec::new();
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut hashes = Vec::new();
    for (recipient, amount, notes) in cases() {
        let data = ext(recipient, amount, [&notes[0], &notes[1]]);
        hashes.push(hex(ext_data_hash(&pool, &data)?));
        recipients.push(recipient.to_string());
        amounts.push(amount);
        first.push(format!("0x{}", hex::encode(&data.notes[0])));
        second.push(format!("0x{}", hex::encode(&data.notes[1])));
    }

    // The contract must accept a signature made over these digests; a mismatch
    // in the EIP-712 domain or type string would be invisible until a live
    // settlement failed.
    let session = Session {
        nullifier: Field::from(7_u64),
        seller: address("0x000000000000000000000000000000000000dEaD"),
        deadline: 1_900_000_000,
    };
    let calls_root = alloy::primitives::keccak256("412 calls");

    let fixture = serde_json::json!({
        "pool": POOL,
        "escrow": {
            "nullifier": 7,
            "seller": "0x000000000000000000000000000000000000dEaD",
            "deadline": session.deadline,
            "amount": 300,
            "callsRoot": format!("{calls_root}"),
            "domainSeparator": format!("{}", pool.domain_separator()),
            "lockDigest": format!("{}", pool.lock_digest(&session)),
            "settlementDigest": format!("{}", pool.settlement_digest(&session, 300, calls_root)),
        },
        "token": TOKEN,
        "verifier": VERIFIER,
        "chainId": CHAIN_ID,
        "tokens": tokens,
        "assetIds": asset_ids,
        "recipients": recipients,
        "amounts": amounts,
        "notes0": first,
        "notes1": second,
        "hashes": hashes,
    });
    fs::write(
        root()?.join("contracts/test/fixtures/binding.json"),
        format!("{}\n", serde_json::to_string_pretty(&fixture)?),
    )?;
    Ok(())
}

/// The lone-leaf path is the zero chain, which the pool's tree and the circuit
/// both derive independently.
#[test]
fn lone_leaf_path_is_the_zero_chain() -> Result<(), Box<dyn StdError>> {
    let path = veil402_sdk::MerklePath::only_leaf()?;
    assert_eq!(path.index, 0);
    assert_eq!(path.siblings[0], Field::ZERO);
    assert_eq!(
        hex(path.siblings[1]),
        "0x2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864"
    );
    Ok(())
}
