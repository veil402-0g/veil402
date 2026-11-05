use alloy::primitives::{B256, keccak256};

/// Pairs are hashed in sorted order, which is what `OpenZeppelin`'s `MerkleProof`
/// recomputes, so a buyer can check one call against a root with the standard
/// verifier instead of a bespoke one.
fn pair(a: B256, b: B256) -> B256 {
    let (low, high) = if a <= b { (a, b) } else { (b, a) };
    keccak256([low.as_slice(), high.as_slice()].concat())
}

fn fold(level: &[B256]) -> Vec<B256> {
    level
        .chunks(2)
        .map(|nodes| if let [a, b] = nodes { pair(*a, *b) } else { nodes[0] })
        .collect()
}

/// Roots the per-call attestations behind a settlement, so one signature covers
/// them all whatever the call count.
#[must_use]
pub fn calls_root(attestations: &[B256]) -> B256 {
    if attestations.is_empty() {
        return B256::ZERO;
    }
    let mut level = attestations.to_vec();
    while level.len() > 1 {
        level = fold(&level);
    }
    level[0]
}

/// What one call cost, and the attestation that justifies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Call {
    /// Price in token units, computed by the meter rather than reported to it.
    pub cost: u64,
    /// Digest of the request, response and any provider attestation.
    pub attestation: B256,
}

/// Returns the siblings proving `index` is in the root, for a buyer checking
/// that one call it made is among the ones it was charged for.
#[must_use]
pub fn calls_proof(attestations: &[B256], index: usize) -> Vec<B256> {
    let mut proof = Vec::new();
    if index >= attestations.len() {
        return proof;
    }
    let mut level = attestations.to_vec();
    let mut position = index;
    while level.len() > 1 {
        if let Some(sibling) = level.get(position ^ 1) {
            proof.push(*sibling);
        }
        position /= 2;
        level = fold(&level);
    }
    proof
}
