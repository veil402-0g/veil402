# 002 — EVM Verifier and Shielded Pool Contract

## Overview

Moves the shielded pool from the reference Anchor program to production Solidity on EVM, with a Groth16 verifier, Poseidon-based incremental Merkle tree (LazyIMT), and end-to-end shield/withdraw on Hedera testnet.

## Verifier

The gnark prover exports a Solidity verifier via `cmd/export-verifier`. The verifier is a single `Verifier.sol` generated from the pinned `.vk`:

```
Verifier.verifyProof(uint[2] a, uint[2][2] b, uint[2] c, uint[] inputs) → bool
```

Public inputs layout: `[root, nullifier0, nullifier1, commitment0, commitment1, asset_id]`.

A companion `cmd/solidity-calldata` tool generates the ABI-encoded calldata from a gnark proof, used in fixture-driven Foundry tests.

## Poseidon Hash Alignment

The circuit (Noir), SDK (Rust `light-poseidon`), and contracts (Solidity) must agree on the same Poseidon parameters. A cross-language corpus tool (`tools/poseidon-corpus`) generates test vectors from the Rust implementation, validated in:

- `circuits/poseidon-check/` — Noir circuit that asserts known hashes
- `contracts/test/Poseidon.t.sol` — Solidity test against the same vectors

The commitment hash-to-field is pinned to `keccak256` truncated to the BN254 scalar field.

## Pool Contract

```
Pool.sol
├── initialize(token, verifier)     → deploys tree, stores token address
├── shield(commitment, amount)      → transferFrom + insert leaf
├── transact(proof, publicInputs)   → verify proof, check nullifiers, insert outputs
└── withdraw(proof, publicInputs, recipient, amount)
                                    → verify + nullify + transfer out
```

### Merkle Tree (LazyIMT)

Depth-20 incremental Merkle tree using Poseidon hashing. Leaves are inserted left-to-right. The root is recomputed on every insertion by hashing up the path from the new leaf to the root.

```
insert(commitment):
    leaves[nextIndex] = commitment
    hash up from leaves[nextIndex] using filledSubtrees[]
    root = new root
    nextIndex++
```

### Nullifier Set

A `mapping(bytes32 => bool)` prevents double-spend. `transact` and `withdraw` reject any proof whose nullifiers are already in the set.

## SDK EVM Transport

```
evm/
├── asset.rs     — ERC-20 token address → asset_id field mapping
├── binding.rs   — alloy bindings for Pool.sol ABI
├── calldata.rs  — proof → Solidity calldata encoding
├── client.rs    — EvmClient: shield, transact, withdraw over JSON-RPC
└── mod.rs       — re-exports
```

The client wraps alloy's `Provider` and `ContractInstance`, mapping SDK types (Note, Transaction, Proof) to the contract's ABI.

## Deployment

Foundry deploy scripts under `contracts/script/`:
- `DeployVerifier.s.sol` — deploys `Verifier.sol`
- `DeployPool.s.sol` — deploys `Pool.sol` with the verifier address

Deployment addresses recorded in `contracts/deployments/296-testnet.json` (Hedera chain ID 296).
