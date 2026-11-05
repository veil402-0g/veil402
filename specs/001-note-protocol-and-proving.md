# 001 — ZK Note Protocol and Proving System

## Overview

A shielded UTXO-based payment layer for metering inference calls. Funds enter the pool as ERC-20 tokens and become **notes** — commitments whose owner, value, and asset are hidden inside a Poseidon hash. Spending a note requires a zero-knowledge proof that the spender knows the preimage without revealing it on chain.

## Note Model

A note is a leaf in a depth-20 Merkle tree. Its commitment is:

```
note_key  = Poseidon(NOTE_KEY_DOMAIN, owner, random)
commitment = Poseidon(NOTE_DOMAIN, note_key, asset_id, value)
```

Spending publishes a **nullifier** that prevents double-spend:

```
nullifier_key = Poseidon(NULLIFIER_KEY_DOMAIN, viewing_key)
nullifier     = Poseidon(NULLIFIER_DOMAIN, nullifier_key, note_key)
```

The owner is derived from two independent secrets:

```
spend_hash = Poseidon(SPEND_KEY_DOMAIN, spending_key)
owner      = Poseidon(OWNER_DOMAIN, spend_hash, nullifier_key)
```

Domain tags (1–10) prevent cross-role collisions — a value hashed as a nullifier key can never be mistaken for a note key.

## Circuit (Noir, BN254)

2-in / 2-out join-split over Poseidon on the BN254 scalar field.

**Public inputs**: Merkle root, 2 nullifiers, 2 output commitments, asset_id.

**Private witnesses**: spending keys, viewing keys, note values, randoms, Merkle paths.

**Constraints**:
1. Each input note's commitment lies in the tree (Merkle membership at depth 20).
2. Each nullifier is correctly derived from the input's viewing key and note key.
3. The sum of input values equals the sum of output values (conservation).
4. The asset_id is consistent across all four notes.

```
fn main(
    root: pub Field,
    nullifiers: pub [Field; 2],
    commitments: pub [Field; 2],
    asset_id: pub Field,
    // private witnesses
    spend_keys: [Field; 2], view_keys: [Field; 2],
    values: [Field; 4], randoms: [Field; 4],
    paths: [[Field; 20]; 2], indices: [Field; 2],
)
```

## Proving Pipeline

```
                Noir circuit
                    │
                    ▼
              ACIR bytecode (.json)
                    │
        ┌───────────┴───────────┐
        ▼                       ▼
  Constraint system (.ccs)   gnark Groth16 setup
        │                       │
        ▼                       ▼
   Proving key (.pk)      Verification key (.vk)
        │
        ▼
  gnark worker (gRPC)
        │
        ▼
  Solidity verifier (for on-chain verification)
```

1. **Noir** compiles the circuit to ACIR.
2. **Sunspot** bridges ACIR → gnark constraint system (BN254).
3. **gnark** runs Groth16 setup, producing `.pk` and `.vk`. These are pinned as versioned artifacts (`transaction-v4`).
4. A **Go gRPC worker** loads the pinned artifacts and proves witnesses on demand.
5. The Rust SDK spawns the worker as a child process and talks to it over stdin/stdout protobuf.

## SDK Architecture

```
veil402-sdk/
├── src/
│   ├── protocol/        # note model — field, encoding, note, transaction
│   │   ├── field.rs     # BN254 scalar wrapper (32-byte big-endian)
│   │   ├── encoding.rs  # borsh helpers
│   │   ├── note.rs      # Owner, Note, nullifier derivation, Poseidon
│   │   └── transaction.rs  # Spend, Send, MerklePath, Transaction, Witness
│   ├── proving/         # proof generation
│   │   ├── artifacts.rs # locates the pinned .pk/.ccs bundle on disk
│   │   ├── witness.rs   # builds the gnark witness from a Transaction
│   │   └── worker/      # spawns the Go prover, protobuf codec
│   ├── client/          # high-level API (Config → Veil → Prepared → Proof)
│   └── lib.rs           # re-exports
└── tests/
    └── proving.rs       # round-trip: construct note → prove → verify public inputs
```

**Key types**:
- `Owner` — spending + viewing key pair; derives the public owner field
- `Note` — (owner, value, asset_id, random) → commitment
- `Transaction` — 2 spends + 2 sends → witness → proof
- `Veil` — client that holds the prover worker and pool state

## Reference Pool Program

An Anchor (Solana) pool included as `reference/` for the EVM port:
- `initialize` — creates the tree and token vault
- `shield` — deposits tokens, inserts a note commitment
- `transact` — verifies proof, nullifies inputs, inserts outputs

The tree uses a depth-20 incremental Merkle construction with Poseidon hashing.
