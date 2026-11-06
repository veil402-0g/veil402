# Contracts

The on-chain half of Veil402: a shielded pool, an escrow layer that turns it into
private payment channels, and a registry of trusted enclaves. Foundry, Solidity
0.8.28, deployed on Hedera.

```
src/Pool.sol              shielded pool, escrow and settlement
src/Verifier.sol          Groth16 verifier, generated from the transaction-v4 vk
src/EnclaveRegistry.sol   which signing key belongs to which enclave image
script/                   deployment
deployments/              addresses and measured gas per network
test/                     Foundry tests, fixtures shared with the Rust SDK
```

The zero-knowledge circuit these contracts verify is explained in
[`../circuits/README.md`](../circuits/README.md). This page covers what the
contracts add on top of it.

---

## The pool

`Pool` holds one ERC-20 and a depth-20 Poseidon Merkle tree of note commitments
(`@zk-kit/lazy-imt`, hashed with `poseidon-solidity`). It is **immutable**: no
proxy, no owner, no upgrade path, no pause.

### State

| | |
|---|---|
| `tree` | every commitment ever created, in order |
| `knownRoot` | every root the tree has had at a transaction boundary |
| `nullifierSpent` | every note that has been spent |
| `escrows` | notes reserved against a seller, keyed by nullifier |

The pool never stores who owns what or how much a note holds. Its token balance
is the sum of all notes, and that is the only total it knows.

### `shield` — the only public step

```
commitment = Poseidon(5, npk, assetId, amount)
```

The depositor sends `npk` (the note key, already blinded) and the amount. The
pool computes the commitment itself, so a depositor cannot insert a note worth
more than it paid. The deposit is public; everything after it is not.

### `transact` — every private move

Takes a proof, a root, two nullifiers, two output commitments and `ExtData`
(recipient, signed amount, two note ciphertexts). The pool:

1. refuses nullifiers that are reserved by a live escrow,
2. checks the root is one it has recorded and both nullifiers are unspent, in
   the field and distinct,
3. computes `publicAmount` and `extDataHash` **itself** and passes them to the
   verifier, so the caller cannot choose them,
4. marks the nullifiers spent and appends the two commitments with their
   ciphertexts,
5. transfers tokens out only if the amount is negative (a withdrawal).

A shielded transfer moves no tokens at all: two nullifiers in, two commitments
out. Sender, recipient and amount stay hidden.

**Every historical root stays valid.** A proof made against an older root still
verifies after other deposits land, so proving never races the chain. This is
safe because a note's nullifier is the same whatever root it is proved against.

### `extDataHash` — binding the proof to its context

```
keccak256(abi.encode(
    "veil402:transact:v5",
    chainid, pool, token, verifier,
    recipient, extAmount,
    keccak256(len0 ‖ note0 ‖ len1 ‖ note1)
)) mod p
```

This is a public input of the proof. It ties a proof to one chain, one pool, one
token and one verifier, and fixes the recipient, the amount and both
ciphertexts. Anyone may broadcast a proof — which is what lets a relay submit it
— but nobody can redirect it, re-price it or swap a ciphertext. The lengths are
prefixed so bytes cannot slide from one ciphertext into the other.

### Note ciphertexts

Each output carries up to 128 bytes, emitted in `NewCommitment`. It is the
note's opening sealed to the recipient's viewing key. The recipient's daemon
scans these events and finds its notes; to everyone else they are noise.

---

## Escrow: a payment channel without moving money

The insight is that an escrow does not need to hold value. A note is already
locked by whoever can produce its nullifier. The pool only has to stop that
nullifier being spent anywhere else for a while.

### `lock`

```
lock(nullifier, enclave, seller, deadline, signature)
```

- Records `Escrow(enclave, seller, deadline, settled = false)` against the
  nullifier. No tokens move, no commitment changes.
- Only the note's owner can compute its nullifier, so in practice only the
  owner can lock it.
- The enclave must have signed `Lock(nullifier, seller, deadline)` (EIP-712), and
  `EnclaveRegistry` must say it is live. Every reservation is attributable to a
  key buyers pin.
- `deadline` is in the future and at most one hour away, which bounds how long a
  mistaken or front-run lock could hold a note.

On chain this reveals that *some* note is reserved for a seller. It does not
reveal whose note, its value, or the session's ceiling.

### Metering happens off chain

While the escrow is live, the buyer makes as many calls as it wants. The
enclave meters each one and keeps an attestation digest per call (request,
response and 0G's TEE signature). There are no transactions per call.

### `settleToNote` — paying privately

```
NoteSettlement(nullifier, sellerCommitment, callsRoot)    // signed by the enclave
```

The settlement is a normal join-split that spends the escrowed note into:

- `outCommitments[0]` — the seller's note, for the metered amount,
- `outCommitments[1]` — the buyer's change.

The pool requires `extAmount == 0` (nothing public moves) and checks the
enclave's signature over the nullifier, **the seller's exact commitment** and
the calls root. The enclave fixes what the seller is owed; the proof shows the
split conserves value; the chain sees neither amount.

`callsRoot` is a sorted-pair keccak Merkle root over every call's attestation,
the same shape as OpenZeppelin's `MerkleProof`. One signature covers a
session of any length, and the buyer can prove any single call was among those
billed.

### `settleToAddress` — paying publicly

The same flow with a negative `extAmount` paid in tokens to `escrow.seller`. The
signature covers `Settlement(nullifier, amount, callsRoot)`. The payment amount
becomes public; the ceiling and the buyer's change do not.

### Timing: no race between seller and buyer

```
 lock ───────────────── deadline ── +60s ──────────▶
      seller may settle  │           │ buyer may spend the note again
                         └─ grace ───┘
```

A nullifier is reserved until `deadline + SETTLE_GRACE`. The seller can settle up
to the deadline; the buyer's note is free only after the grace window. The two
windows never overlap, so a late settlement cannot collide with a refund.

### Settlement rules that matter

- A settlement must spend **exactly one** escrowed nullifier. Two would let one
  enclave signature consume another escrow's note.
- A settled escrow cannot be settled again, and its nullifier is spent.
- Settlement fails after the deadline.
- The recipient of a public settlement must be the seller recorded at lock time;
  an enclave can pay early, but never redirect.

---

## Enclave registry

`EnclaveRegistry` maps a signing key to `keccak256(PCR0)` of the Nitro image
that holds it. The attestation document is verified off chain against the AWS
Nitro root; the registry records the conclusion so that pinning is auditable —
anyone can check which image a key claimed and when it was revoked.

`lock` requires a live enclave. Revoking one stops new escrows; escrows already
locked to it can still settle, because the buyer chose that key and only the
buyer bears that choice. The registry is the only owned contract
(`Ownable2Step`). It cannot touch funds.

---

## What the chain sees

| Call | Public | Hidden |
|---|---|---|
| `shield` | depositor, amount, a commitment | owner of the note |
| `transact` | two nullifiers, two commitments, ciphertexts | sender, recipient, amount, which deposit funded it |
| `transact` (withdrawal) | recipient, amount | which note paid it |
| `lock` | a nullifier, enclave, seller, deadline | buyer, value, ceiling |
| `settleToNote` | a nullifier, calls root, two commitments | buyer, amount paid, change, call count |
| `settleToAddress` | seller, amount paid, calls root | buyer, ceiling, change |

Every call but `shield` is authorised by a proof or an enclave signature, not by
`msg.sender`, so it can be broadcast by a relay and the buyer's address never
appears.

---

## Cross-language agreement

A wrong typehash or hash parameter would mean escrows that can never settle
against an immutable contract, so the tests pin every boundary to fixtures
produced by the Rust SDK:

| Test | Pins |
|---|---|
| `Poseidon.t.sol` | Solidity Poseidon = Rust = Noir, and the empty-tree zero chain |
| `MerkleTree.t.sol` | on-chain tree inserts match the circuit's path |
| `Binding.t.sol` | asset id, `extDataHash`, lock and settlement digests match the SDK; calls root verifies with OpenZeppelin |
| `Verifier.t.sol` | a real proof verifies; a tampered input does not |
| `Pool.t.sol` | shield, transfer, withdraw, replay, roots, ext-data binding |
| `Escrow.t.sol` | lock consent, deadlines, grace, settlement rules |

## Build and test

```bash
make deps      # pinned contract dependencies into contracts/lib
make abi       # refresh the SDK's Pool ABI
make test      # Foundry + Rust, ABI first
```

Testnet addresses and measured gas are in
[`deployments/296-testnet.json`](deployments/296-testnet.json).
