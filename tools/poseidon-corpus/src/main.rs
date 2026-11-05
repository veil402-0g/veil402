//! Emits the Poseidon corpus that the Noir and Solidity checks verify against.
//!
//! Uses light-poseidon directly rather than the SDK, so the three
//! implementations under comparison stay independent.

use std::{env, fs, path::PathBuf};

use ark_bn254::Fr;
use light_poseidon::{Poseidon, PoseidonBytesHasher};

type Element = [u8; 32];

const WIDTH: usize = 17;
const DEPTH: usize = 21;

fn hash(inputs: &[&Element]) -> Element {
    let mut hasher = Poseidon::<Fr>::new_circom(inputs.len()).expect("poseidon width");
    let bytes: Vec<&[u8]> = inputs.iter().map(|value| value.as_slice()).collect();
    hasher.hash_bytes_be(&bytes).expect("poseidon")
}

fn small(value: u64) -> Element {
    let mut element = [0_u8; 32];
    element[24..].copy_from_slice(&value.to_be_bytes());
    element
}

// Chained so the inputs span the field instead of clustering near zero, where
// a parameter mismatch can stay hidden.
fn spread(seed: u64, count: usize) -> Vec<Element> {
    let mut value = small(seed);
    (0..count)
        .map(|step| {
            value = hash(&[&value, &small(step as u64)]);
            value
        })
        .collect()
}

fn array(name: &str, values: &[Element]) -> String {
    let items: Vec<String> = values
        .iter()
        .map(|value| format!("\"0x{}\"", hex::encode(value)))
        .collect();
    format!("\"{name}\": [{}]", items.join(", "))
}

fn toml_array(name: &str, values: &[Element]) -> String {
    let items: Vec<String> = values
        .iter()
        .map(|value| format!("\"0x{}\"", hex::encode(value)))
        .collect();
    format!("{name} = [{}]", items.join(", "))
}

fn main() {
    let root = PathBuf::from(env::args().nth(1).expect("usage: poseidon-corpus <repo root>"));

    let mut a = vec![small(1)];
    let mut b = vec![small(2)];
    a.extend(spread(1, WIDTH - 1));
    b.extend(spread(2, WIDTH - 1));
    let c = spread(3, WIDTH);
    let d = spread(4, WIDTH);

    let t3: Vec<Element> = (0..WIDTH).map(|i| hash(&[&a[i], &b[i]])).collect();
    let t5: Vec<Element> = (0..WIDTH)
        .map(|i| hash(&[&a[i], &b[i], &c[i], &d[i]]))
        .collect();

    let mut zero = [0_u8; 32];
    let mut zeros = vec![zero];
    for _ in 1..DEPTH {
        zero = hash(&[&zero, &zero]);
        zeros.push(zero);
    }

    let sets: [(&str, &Vec<Element>); 7] = [
        ("a", &a), ("b", &b), ("c", &c), ("d", &d),
        ("t3", &t3), ("t5", &t5), ("zeros", &zeros),
    ];

    let json = format!(
        "{{\n  {}\n}}\n",
        sets.iter()
            .map(|(name, values)| array(name, values))
            .collect::<Vec<_>>()
            .join(",\n  ")
    );
    let fixtures = root.join("contracts/test/fixtures");
    fs::create_dir_all(&fixtures).expect("fixture directory");
    fs::write(fixtures.join("poseidon.json"), json).expect("write json");

    let toml = sets
        .iter()
        .map(|(name, values)| toml_array(name, values))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        root.join("circuits/poseidon-check/Prover.toml"),
        toml + "\n",
    )
    .expect("write Prover.toml");
}
