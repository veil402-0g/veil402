# Circuits

The zero-knowledge half of Veil402. One circuit moves all private value; a second
exists only to prove three Poseidon implementations agree.

```
transaction/       2-in / 2-out join-split, Noir, BN254, depth-20 tree
poseidon-check/    Noir leg of the Noir / Rust / Solidity Poseidon agreement check
```

The circuit is compiled to ACIR, proved with **Groth16** by the gnark worker in
[`prover/veil402-gnark`](../prover/veil402-gnark), and verified on chain by
[`contracts/src/Verifier.sol`](../contracts/src/Verifier.sol), which is generated
from the same pinned `transaction-v4` verifying key. Proving runs locally, on
the machine that holds the note secrets; the witness never leaves it.

---

## The idea in one paragraph

Value lives in a public Merkle tree as **commitments**: hashes that hide who owns
a note and how much it holds. To spend, you do not reveal the note. You publish
its **nullifier**, a second hash only the owner can compute, and a proof that
says: *"the notes behind these nullifiers are in the tree, I own them, and the
value I am creating equals the value I am destroying."* The chain checks the
proof, records the nullifiers so they cannot be used again, and appends the new
commitments. It learns nothing else.

---

## Keys and notes

Every hash is Poseidon over the BN254 scalar field, and every hash takes a
**domain tag** as its first input, so a value computed in one role can never be
accepted in another.

| Tag | Role |
|---|---|
| 1 | spending key hash |
| 2 | nullifier key |
| 3 | owner |
| 4 | note key |
| 5 | note commitment |
| 6 | nullifier |
| 7 | asset id (contract side) |
| 8, 9, 10 | session spend / view / blinding (SDK side) |

```
nullifier_key = Poseidon(2, viewing_key)
owner         = Poseidon(3, Poseidon(1, spending_key), nullifier_key)
note_key      = Poseidon(4, owner, random)
commitment    = Poseidon(5, note_key, asset_id, value)
nullifier     = Poseidon(6, nullifier_key, leaf_index)
```

What each piece buys:

- **`random` (the blinding)** makes the commitment *hiding*. Two notes with the
  same owner and value look unrelated, and nobody can guess-and-check an amount.
- **Poseidon is collision resistant**, so a commitment is *binding*: it opens to
  exactly one `(owner, random, asset, value)`. That is what lets one note be
  disclosed to an auditor later without a circuit.
- **Two keys, not one.** The owner commits to the spending key and the viewing
  key separately. The viewing key alone derives nullifiers and finds notes; it
  cannot spend. The spending key is needed to reproduce `owner`.
- **The nullifier uses `leaf_index`, not the commitment**, so it cannot be
  computed from public data, and it is different for every leaf even if two
  notes had identical contents.
- **`asset_id`** is `Poseidon(7, token_address)`, so a note is bound to one token
  and one pool.

---

## The transaction circuit

`transaction/src/main.nr` — a fixed 2-input, 2-output join-split.

### Public inputs (verifier order)

```
root                  a tree root the pool has recorded
nullifiers[2]         spent notes
out_commitments[2]    new notes
public_amount         signed value entering (+) or leaving (−) the pool
ext_data_hash         binds recipient, amount and ciphertexts to this proof
```

### Private witness

Input values, blindings, spending and viewing keys, leaf indices and Merkle
paths; output note keys and values; the asset id; a private copy of
`ext_data_hash`.

### Constraints

For each input `i`:

1. `in_values[i]` fits in 64 bits.
2. Recompute `owner → note_key → commitment` from the private keys.
3. Recompute the Merkle root from the commitment, `siblings[i]` and
   `leaf_indices[i]`.
4. `in_values[i] * (computed_root - root) == 0` — a real input **must** be in the
   tree; a zero-valued input is a dummy and skips membership.
5. `nullifiers[i] == Poseidon(6, nullifier_key, leaf_index)` and is non-zero.

Then:

6. `sum_in != 0` — every proof spends something real.
7. `nullifiers[0] != nullifiers[1]` — no spending one note twice in one proof.
8. Each output fits in 64 bits and matches its public commitment.
9. `sum_in + public_amount == sum_out` — value is conserved.
10. `ext_data_hash == bound_hash` — the public hash is tied to a witness, so it
    is part of the constraint system and cannot be swapped after proving.

### Why each design choice is there

**Fixed 2×2 arity hides the shape.** A one-note payment with change, a two-note
merge and a full withdrawal all produce the same public footprint: two
nullifiers and two commitments. Dummies are zero-valued notes with real keys and
real nullifiers, so they are indistinguishable from real ones on chain.

**Membership is bypassed by multiplication, not a branch.** Circuits have no
`if`. Multiplying the root difference by the value means the constraint holds
for any path when the value is zero, and only for the true root otherwise.

**The leaf index drives both the path and the nullifier.** `merkle.nr` takes the
left/right direction at each level from the bits of `leaf_index`, the same value
hashed into the nullifier. A spender cannot prove membership at one position and
derive a nullifier for another. Noir's fixed-width bit decomposition also
constrains the index to 20 bits, so it cannot wrap.

**Range checks on inputs and outputs.** Field arithmetic wraps at a 254-bit prime.
Without the 64-bit bound, an output of `p − 1` would be "negative one" and could
mint value. Inputs are checked too, as defence in depth, rather than trusting
that every note in the tree was created by a valid proof.

**Withdrawals are field negatives.** `public_amount` is a field element, so
"−1000" is encoded as `p − 1000`. The conservation check works unchanged; the
contract computes that encoding itself rather than trusting the caller.

**Deposits do not use the circuit.** `sum_in != 0` forbids a proof with no
input. A deposit is a plain public `shield` that inserts the commitment directly.

**`ext_data_hash` makes the proof non-malleable.** It commits to the chain id,
pool, token, verifier, recipient, signed amount and both output ciphertexts. A
relayer that broadcasts the proof cannot change where money goes, how much, or
what the recipient can decrypt. A proof for one pool is useless on another.

### What this circuit does *not* do

It does not know about escrows, sellers or enclaves. A metered session is an
ordinary join-split whose input nullifier happens to be reserved; the contract
adds the extra rules. The same circuit and the same trusted setup serve
deposits-out, private transfers, agent-to-agent payments and settlements.

---

## The Poseidon agreement check

Three Poseidon implementations must produce identical outputs, or a note made by
the SDK could never be spent in the circuit or inserted by the contract:

| Where | Implementation |
|---|---|
| Rust SDK | `light-poseidon` (circom parameters) |
| Noir circuit | `poseidon::bn254` |
| Solidity pool | `poseidon-solidity` T3 / T5 |

`tools/poseidon-corpus` generates vectors from Rust into
`contracts/test/fixtures/poseidon.json`. `poseidon-check` asserts the Noir hash
of every vector and the 20-level zero chain of the empty tree; the Foundry
Poseidon tests assert the Solidity side against the same file.

```bash
make poseidon
```

---

## Build and test

```bash
cd circuits/transaction
nargo test        # accepts payment, merge, withdrawal; rejects forgery, double spend,
                  # value creation, out-of-range values, swapped ext data, index overflow
```

The proving artifacts are pinned in `prover/veil402-gnark/artifacts/transaction-v4`.
Changing the circuit means new artifacts, a new verifier and a new pool — the
pool is immutable by design.
