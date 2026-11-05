# Veil402 — EVM

Shielded 2-in/2-out join-split pool, ported from the Solana implementation to EVM.
The circuit, prover and note protocol are unchanged; only the chain transport is new.

## Why the port is cheap

The circuit is Groth16 over **BN254** — the same curve the EVM's `ecPairing` precompile
speaks. The existing verifying key therefore produces a Solidity verifier directly, with
no new circuit and no new trusted setup.

## Layout

```
circuits/transaction/   Noir circuit — unchanged (2-in/2-out, depth 20)
prover/veil402-gnark/   gnark Groth16 worker + pinned transaction-v4 artifacts
  cmd/export-verifier     vk  -> Solidity verifier
  cmd/solidity-calldata   raw proof -> verifier argument layout
crates/veil402-sdk/     chain-agnostic client: notes, witness, proving
contracts/              Foundry: verifier, pool, escrow
docs/reference-spec/    design notes carried over from the Solana build
```

## Public inputs

Seven, in verifier order:

```
[ root, nullifiers[0], nullifiers[1], commitments[0], commitments[1],
  public_amount, ext_data_hash ]
```

The exported verifier also takes a gnark Pedersen commitment and its proof of knowledge:

```solidity
verifyProof(uint256[8] proof, uint256[2] commitments,
            uint256[2] commitmentPok, uint256[7] input)
```

## Phase 0 — reproduce

```sh
# 1. export the Solidity verifier from the pinned verifying key
cd prover/veil402-gnark
go run ./cmd/export-verifier \
  -vk artifacts/transaction-v4/transaction.vk \
  -out ../../contracts/src/Verifier.sol

# 2. produce a real proof and its public inputs
cd ../..
cargo test --manifest-path crates/veil402-sdk/Cargo.toml --test proving

# 3. convert the proof into verifier calldata
cd prover/veil402-gnark
go run ./cmd/solidity-calldata \
  -fixture ../../contracts/test/fixtures/transfer.json \
  -out ../../contracts/test/fixtures/calldata.json

# 4. verify it on a local EVM
cd ../../contracts && forge test -vv
```

## Status

| Check | Result |
|---|---|
| Solidity verifier exported from `transaction-v4` vk | done |
| Noir Poseidon matches circomlib (`hash_2`, `hash_3`, `hash_4`) | done |
| SDK builds with no Solana dependency | done |
| Real proof generated (388 bytes) | done |
| Proof verifies on a local EVM | **done — 395,368 gas** |
| Tampered public input is rejected | done |

### Gotcha worth keeping

The exported verifier derives the Pedersen commitment's public input with
`keccak256`. gnark's prover defaults to a different hash-to-field, so the
worker pins both sides to keccak via `WithProverHashToFieldFunction` /
`WithVerifierHashToFieldFunction`. Without that the pairing check fails with
`ProofInvalid()` even though every point is valid.
