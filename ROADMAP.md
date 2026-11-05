# Veil402 — roadmap

ETHGlobal Mumbai. Shielded, mandate-governed payments for AI agents, with
delivery proof.

---

## 1. The product

A company funds a shared shielded pool and issues each agent a private,
revocable mandate. An agent spends by proving — directly to a Hedera contract —
that it holds unspent budget and an unrevoked mandate. It cannot exceed its
allocation, and two agents cannot race the same rupee.

It locks payment in escrow against an x402 inference endpoint running inside a
Nitro enclave whose PCRs the agent verifies before paying, so the seller can
never read the prompt. The enclave calls 0G Compute, returns the response
encrypted to the agent, and only then claims the escrow by presenting 0G's TEE
attestation. No attestation, no payment; a timeout refunds the buyer.

Every spend writes a sealed receipt to a Hedera HCS topic that only the company
can read. The company can later prove one agent's activity to an auditor,
provably complete, without revealing any other agent.

**Why it is defensible:** every shipped agent-spend product (Coinbase Agentic
Wallets, Nevermined, Skyfire, ERC-7710, SmartSessions) buys control by
centralising visibility — in the chain, or in a platform. None offers control
without disclosure.

### Load-bearing sponsors

| Sponsor | Role | Remove it and |
|---|---|---|
| Hedera | settlement, escrow, HCS receipts | no chain |
| 0G | the thing bought; its attestation gates the escrow | the escrow has no release condition |
| EF | anon-RPC transport, CROPS | privacy is partial |

---

## 2. Architecture decisions

| Decision | Reason |
|---|---|
| **Company self-hosts everything** (`npx veil402 init`) | Proving needs the witness, which holds note secrets. A hosted prover would see them. Friction lands on the org, not the developer — their devs still just get a URL and a key |
| **One multi-tenant Enclave B** for providers | Nitro runs on dedicated EC2; one enclave per provider is economically impossible. One image means one PCR set for buyers to pin |
| **Provider keys sealed to `PCR0` via KMS** | Provider never handles a key; we cannot decrypt it either. Their only job is topping up 0G |
| **Single escrow release signer** | Accepted bound: a compromise can pay a seller early, never redirect funds — `escrow.seller` is fixed by the buyer at lock time |
| **Escrow scheme, not `exact`** | Deliberate. See risk R2 |
| **Hedera, not 0G Chain** | bn254 precompiles confirmed present and measured; 0G Chain unverified |
| **Pool denominated in a token, never native HBAR** | Avoids the 8-vs-18 decimal boundary entirely, and native `CryptoTransfer` bypasses `receive()`/`fallback()` |
| **Pool immutable, forever** | Tornado Nova's Gnosis deployment sat behind a governance proxy; the 2023 takeover could have drained it |

---

## 3. What exists

Ported from the Solana implementation at `~/Desktop/veil402`, unchanged except
for the transport.

```
circuits/transaction/     Noir 2-in/2-out join-split, BN254, depth 20
prover/veil402-gnark/     gnark Groth16 worker + pinned transaction-v4 artifacts
  cmd/export-verifier       vk -> Solidity verifier
  cmd/solidity-calldata     raw 388-byte proof -> verifier argument layout
crates/veil402-sdk/       chain-agnostic: notes, witness, proving
circuits/poseidon-check/  Noir leg of the three-way Poseidon check
tools/poseidon-corpus/    corpus generator, excluded from the workspace
contracts/                Foundry
```

### Circuit facts

```
public inputs (7, in verifier order):
  root, nullifiers[0], nullifiers[1],
  out_commitments[0], out_commitments[1], public_amount, ext_data_hash

verifier signature:
  verifyProof(uint256[8] proof, uint256[2] commitments,
              uint256[2] commitmentPok, uint256[7] input)

raw proof = 388 bytes = Ar(64) + Bs(128) + Krs(64) + count(4)
                        + commitment(64) + pok(64)
```

Deposits cannot go through the circuit — `assert(sum_in != 0)` forbids a proof
with no input. Shield is a direct commitment insert.

`public_amount` is signed, encoded as `p - |amount| mod p` for negatives. The
circuit's `sum_in + public_amount == sum_out` depends on that wraparound.

Dummy inputs work via `in_values[i] * (computed_root - root) == 0`: a
zero-valued input bypasses membership, so 1-in and 1-out shapes pad to 2x2
without revealing arity.

### Constants

```
TREE_DEPTH         20          NOTE_DOMAIN       5
ASSET_DOMAIN       7           MAX_NOTE_BYTES    128
BINDING_DOMAIN     "veil402:transact:v5"
r = 21888242871839275222246405745257275088548364400416034343698204186575808495617
```

### Library stack — write as little as possible

| Need | Source |
|---|---|
| Groth16 verifier | generated from `transaction-v4` vk |
| Fixed-depth Poseidon tree | `@zk-kit/imt.sol` `BinaryIMT`, audited via Semaphore |
| Poseidon T3 / T5 | `poseidon-solidity` |
| Reentrancy, SafeERC20 | OpenZeppelin |

`Pool.sol` should be roughly 120 lines.

---

## 4. Measured, not estimated

On Hedera testnet unless noted.

```
verifyProof, full transaction        378,716 gas   (~348k execution)
  same on anvil                      395,089       Hedera is ~12% cheaper
PoseidonT3, full transaction          39,573
PoseidonT3 warm, in-process           18,877       (anvil)
insert = 20 x T3                     378,245       (anvil)
Verifier deploy                    1,992,722
PoseidonT3 deploy                  5,152,328

projected transact                     ~1.2M       inserts are ~60%, not the pairing
projected shield                       ~490k
```

Hedera gas is USD-pegged at $0.0000000852 plus a 20% surcharge. **Envelopes
should be $5+** — at $2 the round trip is ~10% overhead.

### Deployed (chain 296)

```
Verifier     0x42532488f967329e2B090dCf68396Ef0dd63de69   Sourcify exact_match
PoseidonT3   0x98e7e0cc3a12183af743a9d33ca0dfa0982197fc   23,576 B on-chain
proof tx     0x941a3b7c91f397899d72fea4319f3730a0cc70cb1b13a1635473dec7b00fc6e2
```

---

## 5. Gotchas already paid for

**gnark commitment hash-to-field.** The exported verifier derives the Pedersen
commitment's public input with keccak256; gnark's prover defaults to something
else. Both sides are pinned with `WithProverHashToFieldFunction` /
`WithVerifierHashToFieldFunction`. Without it the pairing fails with
`ProofInvalid()` even though every point is valid.

**Optimizer runs is a 37% cliff, not a curve.** Below ~50k the Poseidon round
loop is not unrolled: 16,466 B / 600,505 gas per insert. Above: 23,576 B /
378,245. We match upstream's `2**32-1`. That leaves PoseidonT3 1,000 bytes under
EIP-170, guarded by a size test.

**Poseidon is circomlib-compatible.** Noir's `hash_2`/`hash_3`/`hash_4` map to
`PoseidonT3`/`T4`/`T5`. Verified three ways over a shared corpus. `zeros[1]`
matches the Solana fixture `0x2098f5fb…`.

**Never `delegatecall` into `0x167`** or any Hedera system contract — shared
state, a real 2023 exploit.

**Mirror node is the source of truth.** The relay's `eth_getLogs` is a thin
wrapper over it. 60-day query window, 100 rows per page, cursor pagination — the
daemon must keep a persistent index and backfill, never rebuild from genesis.

**Relay: use QuickNode.** Hashio 502s intermittently and once reported a
successful deploy as failed; thirdweb cannot serve precompile account state, so
`forge script` fails against it. The team's QuickNode app serves
`hedera-testnet` and answers state for `0x06`/`0x07`/`0x08`, so `forge script`
works. Endpoint in `.env.testnet`. Alchemy supports Hedera but the network is
not enabled on that app, and dRPC has no Hedera at all.

**Pin `alloy-rlp-derive`.** A fresh lock resolves it to 0.3.17 against
`alloy-rlp` 0.3.16, and the mismatched pair fails to compile inside
`alloy-eip7702`. garden-rs has the matched 0.3.16 pair.

**Docker on this machine.** A stale shell alias points `docker` at
`/Applications/Docker.app/…`; it actually lives in `~/Applications`. Solo
(which replaces the deprecated `hedera-local-node`) is viable once that alias
is removed.

---

## 6. Phases

Phase 0 is complete: verifier exported, Poseidon verified, a real proof
verifying on Hedera.

### Phase 1 — pool (complete)

A full round trip is live on Hedera testnet: shield 561,202 gas, transact
992,416 — a real Groth16 proof through the deployed verifier, moving ERC20
value. anvil agrees within 12 gas, so the inner loop is trustworthy.

The spend path uses `MerklePath::only_leaf`, valid only while the tree holds a
single note. Spending any later note needs the commitment index and its
siblings, which is the indexer in Phase 3.

| # | Task | Gate |
|---|---|---|
| 1.3 | `LazyIMT`, zero = 0 | done — zero chain matches `0x2098f5fb…` |
| 1.4 | `Pool.sol` | done — round-trips; replay, duplicate nullifier, zero root, unknown root, tampered extData all revert |
| 1.5 | `sdk/src/evm/` — `asset`, `binding`, `calldata`, `client` | done — Rust and Solidity agree on asset ids and ext-data hashes |
| 1.6 | Integration + measured gas | done — shield + transact on Hedera testnet |

**Unblocked.** QuickNode `hedera-testnet` replays the deployed verifier
(`eth_call` clean, estimate 407,273) and `forge script` simulates chain 296
end-to-end. 40/40 calls, mean 902ms.

**Tree is `LazyIMT`, not `BinaryIMT`.** Same zk-kit family, roots verified
identical over 128 leaves, 41% cheaper. `BinaryIMT` rewrites `lastSubtrees`
(two slots) at all 20 levels on every insert; `LazyIMT` writes only the nodes
that change and derives the root on read. Per pool transaction — two
commitments and one root read — `BinaryIMT` averages 826,771 gas against
`LazyIMT`'s 488,753. The catch: `_root()` with no argument sizes the tree to
its current leaf count, so the pool must always call `_root(20)`.

#### `Pool.sol` design

```
storage
  mapping(uint256 => bool) knownRoot        unbounded, no eviction
  mapping(uint256 => bool) nullifierSpent
  LazyIMTData tree                          depth 20, zero = 0
  immutable token, verifier
  immutable assetId = PoseidonT3(ASSET_DOMAIN, uint160(token))

shield(npk, amount, note)
  contract derives PoseidonT5(NOTE_DOMAIN, npk, assetId, amount)

transact(proof, root, nullifiers, outCommitments, extAmount, extData) nonReentrant
   1  extAmount <= 0
   2  root, nullifiers, outCommitments all < r
   3  nullifiers non-zero
   4  nullifiers distinct
   5  notes <= 128 bytes
   6  recipient != address(this)
   7  neither nullifier spent            reads
   8  root != 0 && knownRoot[root]
   9  extDataHash = keccak(v5 fields) % r    derived, never a parameter
  10  publicAmount = fieldEncode(extAmount)
  11  verifyProof
  ----------------------------------------- effects
  12  mark both nullifiers
  ----------------------------------------- interactions
  13  insert both leaves, record final root once
  14  safeTransfer(recipient, -extAmount)
      emit Nullified x2, then NewCommitment x2
```

Built with the transfer last, after the inserts, so every effect precedes the
only external call. `_root(20)` is 20 Poseidon delegatecalls, so the root is
derived once per call rather than once per leaf: 628,219 gas against 1,024,324.
Only transaction boundaries are provable states, so skipping the intermediate
root costs nothing. Measured with a stub verifier; the real pairing adds ~378k.

`shield` takes `uint64` amounts — the circuit range-checks note values to 64
bits, so an 18-decimal token would cap near 18 units. Fee-on-transfer and
rebasing tokens are unsupported.

#### The SDK binding

`plan()` pads and shuffles outputs with `OsRng`, so the ciphertext order does
not exist until planning runs. `transfer` and `withdraw` therefore own the whole
sequence — plan, bind, prove, encode — and return `Prepared { proof, to,
calldata }`. The order is held in one scope, so proof and call cannot disagree
and nothing has to be checked back. This is the Solana shape; an earlier
`ExternalData` callback with the ciphertexts carried on `Proof` was more
machinery for the same guarantee.

`Withdrawal::new` rejects zero, burn and unrepresentable amounts up front, so a
withdrawal that cannot settle never reaches the prover.

The binding covers chain, pool, token, verifier, recipient, amount and both
length-prefixed ciphertexts under `keccak256("veil402:transact:v5")`. Token and
verifier are immutables implied by the pool address, so binding them is
redundant on EVM — it costs ~12 gas and keeps one binding shape across both
chains.

Bindings are generated from the compiled ABI (`make abi`, following garden-rs),
so a Rust signature cannot drift from the contract and `ExtData` has one
definition rather than two.

Agreement is a fixture both sides read: Rust writes `binding.json`, the Solidity
test etches the pool at the fixture's address and asserts. The Rust proof parser
is checked against the Go encoder over a frozen proof — not the live fixture,
which the proving test rewrites with fresh randomness.

**Unbounded root mapping, not a ring buffer.** Privacy Pools shipped a 30-slot
buffer and took audit finding AW-M-02: an attacker spams deposits between proof
generation and submission, evicting the user's root. They raised it to 64, which
raises the cost without removing the class. Our proving takes 5–10s and Hedera
blocks are ~2s, so the window is wide. One extra SSTORE per insert removes the
whole class.

**v5 ext-data binding** adds `block.chainid`; v4 did not, and EVM proofs replay
across chains. Ciphertexts are length-prefixed to stop byte-shifting between
them.

Nullifier distinctness and unspent-ness are first-class checks — Solana got them
free from PDA collision; Solidity has no equivalent.

Duplicate commitments are legal. Do not add a uniqueness check.

### Phase 2 — escrow (2.1-2.9 done)

`lock(nullifier, enclave, seller, deadline)` · `settleToAddress(...)` ·
`settleToNote(...)`.

**The pool never holds the escrowed value.** It reserves a nullifier. The note
stays the buyer's; it simply cannot be spent elsewhere while the reservation is
live, and when the deadline passes the reservation lifts. There is no refund
function and nothing to get stuck.

Only the note's owner can name its nullifier, so nobody can lock a note they do
not hold. `A <= L` needs no check either — the circuit cannot let outputs exceed
inputs, so overpaying is arithmetically impossible.

`settleToAddress` pays the seller publicly: the ceiling and the buyer's change
both stay hidden, and only the amount paid is visible. `settleToNote` pays a
note against a commitment the enclave signed, so the amount never becomes
public — shipped but unusable until note encryption and discovery exist, which
is why both paths are in the contract now. The pool is immutable; adding the
second path later would mean redeploying.

Both are the same join-split underneath, so `_spend` is shared and `transact`'s
signature is unchanged.

**Grace window.** The seller may settle up to the deadline; the buyer regains
the note only at `deadline + 60s`. Without it a settlement landing late would
collide with the buyer taking the note back, and the seller would lose paid-for
work through no fault of its own.

`lock` is gated on `EnclaveRegistry` and carries an enclave signature over
`(nullifier, seller, deadline)`. The registry is checked at lock only — checking
it at settle would let a revocation strand a live escrow, costing the seller
work it had already delivered. The
enclave never needs gas or chain access — anyone may submit it — but a
reservation now requires the enclave's consent, so it cannot be made against an
enclave that will not serve, and every reservation is attributable to a key
buyers pin and we can revoke.

`EnclaveRegistry` records which signing key belongs to which `PCR0`. Gating
`lock` on it narrows the grief surface from anyone to keys we registered. Its
blast radius is freezing notes, never theft: settle still needs a proof only the
note's owner can produce.

Measured: `lock` ~66k, `settleToAddress` ~726k, `settleToNote` ~696k, and the
reservation check costs `transact` 5,019 gas (992,416 -> 997,435).

`evm::escrow` carries the EIP-712 digests, and `veil402-enclave` holds the
meter: it counts what it actually sent and received rather than believing a
provider's figures, and signs a running total after each call. Vouchers are
cumulative, so the seller keeps only the newest and a buyer who walks away
mid-session still pays for what it consumed.

Agreement is end to end rather than by inspection — the Solidity test signs the
digest the SDK produced and the pool accepts it. Flipping one hex digit gives
`WrongEnclave`.

Live on Hedera testnet: lock 84,864 gas, settle 1,011,633, three checkpoints
costing nothing on chain.

Remaining: 2.10, which needs a real HTS token in the account before it can run.

### Phase 3 — mandates (5h)

Shared Semaphore group across companies with per-company write ACL — a
per-company group would name the company in every lock proof. Revocation is a
leaf removal and a new root. Already-issued capability survives until expiry;
token TTL is the dial.

Reuse ERC-7710 delegation structs and caveat semantics as the mandate vocabulary
rather than inventing a format; the hash becomes the Semaphore leaf, and the
object stays publicly enforceable through the real Delegation Manager in a
dispute.

### Phase 4 — `veil-daemon` (8h)

OpenAI-compatible API, agent keys, envelope sessions (cold path proves and
locks, warm path does neither), note selection with re-split at lock time to
self-defragment, receipts. Persistent local index — the mirror node's 60-day
window makes rebuild-from-genesis impossible.

### Phase 5 — enclave logic, stub attester (6h)

Rust behind an `Attester` trait: `StubAttester` locally, `NitroAttester` on EC2.
Decrypt, call 0G, spend ceiling, encrypt response, sign release, multi-tenant key
handling. Nitro cannot run on macOS, so everything but attestation and KMS is
testable locally.

### Phase 6 — real 0G (4h)

`getRequestHeaders(provider, content)` signs over the prompt, so the 0G wallet
key must live in the enclave. Attestation is EIP-191 `personal_sign` over the
**response**, verified against `teeSignerAddress` from the on-chain Service
contract.

Because it signs the response and not the request, the escrow verifies the
**enclave's** signature binding `(escrowId, requestHash, responseHash,
zeroGSigHash)`. The 0G signature is the enclave's input and the buyer's
independent evidence. Without this a seller could replay any attestation.

**R14 guards are mandatory** — see risks.

### Phase 7 — x402 + PCR (4h)

`scheme: "escrow"` 402 challenge; PCRs pinned in the SDK and verified
automatically, like certificate pinning. The buyer must refuse to lock against
wrong PCRs.

### Phase 8 — receipts and audit (5h)

HCS topic carrying three message types:

```
SPEND     Gateway, plaintext    { nf, amount, service, ts }
RECEIPT   Agent, sealed         { agentId, service, amount, nf, providerRequestId,
                                  attestationHash, ts }
EPOCH     Console, plaintext    { epochRoot, from, to, count }
```

Every `SPEND` must have a matching `RECEIPT` with the same nullifier — an agent
that spends and stays silent leaves a visible, counted gap. Disclosure proves
completeness via the epoch root, not just authenticity.

No `submitKey` on the topic: requiring one would give every agent a Hedera key
and that key becomes a linkage. Junk does not decrypt and costs the spammer
money.

### Phase 9 — company dashboard (6h)

Fund, agents, revoke, live activity, audit export, embedded playground.

### Phase 10 — provider portal (5h)

Privy auth. Create account generates a 0G wallet inside the enclave, sealed
under a KMS policy on `PCR0`. Provider funds the address and sets prices. They
never see a key.

### Phase 11 — Nitro on EC2 (5h)

EIF build, real attestation, KMS `PCR0` policy, vsock relay for the outbound TLS
paths. Publish PCRs. The release key must be KMS-sealed, not ephemeral, or a
crash strands open escrows until timeout.

### Phase 12 — `npx veil402 init` (3h)

Fetches native binaries and the pinned artifacts, generates keys, starts the
daemon and local dashboard.

### Phase 13 — demo (7h)

Per-judge sandbox from a pre-warmed container pool with wildcard routing (4h),
background traffic so the anonymity set is real during judging (1h), fallback
recording and polish (2h).

---

## 7. Risks

| # | Risk | State |
|---|---|---|
| R1 | **0G SDK R14** — fee computed from unauthenticated provider token counts, signature checked *after* billing, skipped entirely without `chatID`. Open upstream, High | Mitigate in Phase 6: per-request ceiling, mandatory `chatID`, verify-before-bill, provider allowlist |
| R2 | **x402 eligibility.** Hedera ships the `exact` scheme with `@x402/hedera` and Blocky402; the track says "via an approved facilitator". We build `escrow`, an open proposal | Parked by decision. Insurance is ~half a day: add `exact` for compliance, keep escrow as the differentiator |
| R3 | Relay reliability | Resolved — QuickNode `hedera-testnet`. Shared team app, so a vendor-side key change is still a single point of failure |
| R4 | Same-entity binding across two proofs is weak — pool proof and mandate proof could come from different parties. Threat is collusion inside one company | Documented; a merged circuit is the real fix |
| R5 | Anonymity set is the pool size, near zero at launch | Background traffic in Phase 13 |
| R6 | 16GB host caps Docker at ~11GB; Solo wants 12GB | Testnet instead |
| R7 | PoseidonT3 is 1,000 bytes under EIP-170 | Size test; solc pinned at 0.8.28 |
| R8 | `transact` is now escrow-aware — the reservation check sits on the core spend path, in an immutable contract | All 21 pre-existing Pool tests pass unchanged; the real anvil round trip still settles |
| R9 | A front-run `lock` can strand someone else's note | Largely closed. `lock` is registry-gated and needs enclave consent, so only a key we registered can do it; bounded by `MAX_LOCK` = 1 hour; needs the owner's nullifier, secret until spent. The clean fix is an ownership proof at lock — a circuit change, Phase 8 |
| R11 | The registry owner can refuse new enclaves, stopping new sessions | Escrow only. `shield` and `transact` never read the registry, live escrows settle regardless, and no funds can be stranded |
| R10 | `settleToNote` ships untested against a live seller — no note encryption or discovery exists yet | Contract-level tests only. Do not advertise it until Phase 3 |

---

## 8. Working rules

- **Never commit without explicit review.** Present first.
- Reuse before hand-rolling. A paragraph-long comment is usually a missing library.
- Comments: default to none, one line, never explaining a fix. See `CLAUDE.md`.
- No test code in `src/`.
- Per-task: run only the touched tests. Full gate at phase close.
- Each phase ends with a standards pass before the commit.

### Cut ladder

```
13  background traffic          cut first
13  sandbox pool -> 3 fixed demo companies
12  npx wizard -> documented docker run
10  provider portal -> seeded, read-only
 9  dashboard polish
 7  PCR pinning -> verify manually on stage
------------------------------------------- never below
    pool · escrow+change · enclave+0G · audit bundle
```

### Commands

```sh
make poseidon     # three-way Poseidon agreement
make test         # everything
cd contracts && forge test
cargo test --manifest-path crates/veil402-sdk/Cargo.toml
```

Testnet credentials live in `.env.testnet`, gitignored. Chain 296, mirror
`https://testnet.mirrornode.hedera.com/api/v1`.
