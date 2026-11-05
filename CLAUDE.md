# Veil402 — working rules

## Reuse before hand-rolling

Check for an audited library before writing a primitive. Current stack:

| Need | Source |
|---|---|
| Groth16 verifier | generated from `transaction-v4` vk by `cmd/export-verifier` |
| Poseidon T3 / T5 | `poseidon-solidity` |
| Fixed-depth Merkle tree | on chain `@zk-kit/lazy-imt` — `LazyIMT`, read via `_root(20)`; off chain `rs-merkle-tree` |
| Reentrancy, SafeERC20 | OpenZeppelin |
| EVM types, ABI, RPC | `alloy` umbrella crate; bindings via `sol!(…, "abi/pool.json")` |
| EIP-712 typed data | `alloy::sol!` + `SolStruct::eip712_signing_hash` — never a hand-written typehash or domain tag |
| Canonical serialisation | `borsh` derive — deterministic, and rejects trailing bytes |
| Sealed envelope to a public key | `hpke`, RFC 9180 |
| Signing, recovery | `alloy` — `SignerSync`, `recover_address_from_prehash` |

**Read `Cargo.toml` before writing a primitive.** A dependency that is declared
and unused is a signal, not an accident: `borsh` sat in this manifest untouched
while a byte codec was hand-rolled beside it.

Stop and look for the crate if you are writing any of these: offset arithmetic
over a wire format, a length check on one, a typehash string, a
domain-separation tag, a base64 table. Each already exists in the tree, and the
hand-written version is where the bug will be — a wrong typehash means escrows
that can never settle against an immutable contract.

If explaining a block of code takes a paragraph, that is usually a missing
library, not a missing comment.

## Comments

Default to none. Explain **why**, never **what**.

- One line, or nothing. A doc comment with a blank `*` line is too long.
- Never comment a fix. The code is the fix; rationale goes in docs.
- No dev-process breadcrumbs: no task or phase numbers, no "used to be",
  no "arrives later", no spec section references.
- Error strings stay precise — those are user-facing.

This is a first-draft constraint, not a cleanup pass.

## Chain

Hedera. Solo for local (pinned), anvil for the fast inner loop, Solo as the
gate. Foundry pinned — 1.8.x breaks against the Hiero relay.

Never `delegatecall` into `0x167` or any system contract.

## EVM bindings

Generated from the compiled ABI, never hand-written. `make abi` refreshes
`crates/veil402-sdk/abi/pool.json` and `make test` depends on it, so a changed
contract cannot leave a stale signature behind.
