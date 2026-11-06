# Veil402

Private payments for AI inference.

Veil402 is a shielded pool that lets an agent pay for inference without revealing who is paying, how much, or which model it called. Funds enter the pool as ERC-20 tokens, become hidden notes inside a Poseidon Merkle tree, and leave only when a zero-knowledge proof says they can.

## How it works

A buyer locks a note into an **envelope** — a time-bounded escrow held by the pool contract. Every inference call inside that envelope is metered off-chain by a sealed enclave; only the final settlement touches the chain. One Groth16 proof per envelope, not one per call.

The enclave cannot see the buyer's balance, and the relay cannot see the prompt or the answer. Both are HPKE-sealed to keys the enclave derives from a root seed the registry attests. The chain sees nullifiers and commitments — nothing else.

## What's in the box

- **Circuit** — 2-in/2-out join-split over BN254 Poseidon, depth-20 Merkle tree, written in Noir
- **Prover** — gnark Groth16 worker bridged from ACIR via Sunspot, pinned artifacts (`transaction-v4`)
- **SDK** — Rust client covering notes, witnesses, proving, and the EVM transport
- **Contracts** — Solidity pool with `shield`, `transact`, `withdraw`, an enclave registry, and a Groth16 verifier exported from the circuit's own verification key
- **Daemon** — holds the buyer's notes, serves an OpenAI-compatible API, manages envelopes
- **Gateway** — the seller's enclave: meters calls, relays to upstream, settles against the pool
- **Console** — operator dashboard for agents, mandates, escrows, and deposits
