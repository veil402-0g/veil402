use std::{env, error::Error as StdError, fs, path::Path, process::Command};

use tempfile::tempdir;
use veil402_sdk::{
    Config, Error, Field, MerklePath, Note, Owner, PublicInputs, Secret, Send, Spend, TREE_DEPTH,
    Transaction, Veil,
    evm::{PoolClient, Withdrawal},
};

fn field(value: u64) -> Field {
    Field::from(value)
}

fn secret(value: u64) -> Result<Secret, Error> {
    Secret::from_bytes(field(value).to_bytes())
}

fn owner(spend: u64, view: u64) -> Result<Owner, Error> {
    Ok(Owner {
        spend: secret(spend)?,
        view: secret(view)?,
    })
}

fn spend(asset: Field) -> Result<Spend, Error> {
    Ok(Spend {
        note: Note {
            value: 1_000,
            asset,
            random: secret(33)?,
        },
        owner: owner(11, 22)?,
        merkle: MerklePath {
            index: 0,
            siblings: [Field::ZERO; TREE_DEPTH],
        },
    })
}

fn send(value: u64, seed: u64) -> Result<Send, Error> {
    let byte = u8::try_from(seed).map_err(|_| Error::Transaction("test seed exceeds one byte"))?;
    Send::new(field(seed), value, secret(seed + 1)?, vec![byte; 32])
}

fn transaction(asset: Field, outputs: &[(u64, u64)]) -> Result<Transaction, Error> {
    Transaction::new(
        vec![spend(asset)?],
        outputs
            .iter()
            .map(|(value, seed)| send(*value, *seed))
            .collect::<Result<_, _>>()?,
    )
}

fn root() -> Result<&'static Path, Box<dyn StdError>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "workspace root is unavailable".into())
}

fn artifacts() -> Result<std::path::PathBuf, Box<dyn StdError>> {
    Ok(root()?.join("prover/veil402-gnark/artifacts/transaction-v4"))
}

#[test]
fn public_inputs_have_one_canonical_order() {
    let inputs = PublicInputs {
        root: field(1),
        nullifiers: [field(2), field(3)],
        commitments: [field(4), field(5)],
        amount: field(6),
        hash: field(7),
    };
    assert_eq!(
        inputs.fields(),
        [
            field(1),
            field(2),
            field(3),
            field(4),
            field(5),
            field(6),
            field(7),
        ]
    );
}

#[test]
fn transaction_rejects_unsupported_arity() -> Result<(), Error> {
    assert!(matches!(
        Transaction::new(Vec::new(), Vec::new()),
        Err(Error::Transaction("transaction requires one or two spends"))
    ));

    let asset = field(1);
    assert!(matches!(
        Transaction::new(
            vec![spend(asset)?],
            vec![send(1, 10)?, send(1, 20)?, send(1, 30)?],
        ),
        Err(Error::Transaction("transaction supports at most two sends"))
    ));
    Ok(())
}

#[test]
fn rejects_corrupt_artifacts() -> Result<(), Box<dyn StdError>> {
    let directory = tempdir()?;
    let source = artifacts()?;
    fs::copy(
        source.join("manifest.json"),
        directory.path().join("manifest.json"),
    )?;
    let mut program = fs::read(source.join("transaction.json"))?;
    let first = program
        .first_mut()
        .ok_or("compiled circuit artifact is empty")?;
    *first ^= 1;
    fs::write(directory.path().join("transaction.json"), program)?;

    let Err(error) = Veil::open(Config::new("unused", directory.path())) else {
        return Err("corrupt artifact was accepted".into());
    };
    assert!(matches!(error, Error::Artifacts(_)));
    Ok(())
}

/// Proves the shape the EVM escrow uses: one input note spent, one change note
/// created, the difference leaving the pool as a negative public amount.
///
/// Refreshing the Solidity fixture is opt-in. Proving draws fresh randomness,
/// so writing it on every run left a tracked file dirty after each test.
#[tokio::test]
async fn proves_and_writes_evm_fixture() -> Result<(), Box<dyn StdError>> {
    let repository = root()?;
    let worker = repository.join("target/veil402-gnark-test");
    let status = Command::new("go")
        .args(["build", "-o"])
        .arg(&worker)
        .arg(".")
        .current_dir(repository.join("prover/veil402-gnark"))
        .status()?;
    if !status.success() {
        return Err("Go worker build failed".into());
    }

    let veil = Veil::open(Config::new(worker, artifacts()?))?;
    let pool = PoolClient::new(
        "0x42532488f967329e2B090dCf68396Ef0dd63de69".parse()?,
        "0x0000000000000000000000000000000000000abc".parse()?,
        "0x98e7E0cC3a12183Af743a9D33ca0Dfa0982197FC".parse()?,
        296,
    )?;
    // 1000 in, 700 change out, 300 leaving the pool to the recipient.
    let prepared = veil
        .withdraw(
            &pool,
            transaction(pool.asset(), &[(700, 66)])?,
            Withdrawal::new("0x000000000000000000000000000000000000dEaD".parse()?, 300)?,
        )
        .await?;
    let proof = prepared.proof;

    assert!(!prepared.calldata.is_empty());
    assert_eq!(proof.bytes.len(), 388);
    assert_eq!(proof.artifact, "transaction-v4");
    assert_ne!(proof.public.nullifiers[0], proof.public.nullifiers[1]);

    let inputs: Vec<String> = proof
        .public
        .fields()
        .iter()
        .map(|field| format!("0x{}", hex::encode(field.to_bytes())))
        .collect();
    let fixture = format!(
        "{{\n  \"artifact\": \"{}\",\n  \"proof\": \"0x{}\",\n  \"input\": [\n    \"{}\"\n  ]\n}}\n",
        proof.artifact,
        hex::encode(&proof.bytes),
        inputs.join("\",\n    \"")
    );
    if env::var_os("VEIL_WRITE_FIXTURE").is_some() {
        let out = repository.join("contracts/test/fixtures");
        fs::create_dir_all(&out)?;
        fs::write(out.join("transfer.json"), fixture)?;
    }
    Ok(())
}
