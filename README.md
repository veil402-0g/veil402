# Veil402

**A private routing layer for agentic payments and inference.**

Veil402 lets AI agents, and the people and businesses running them, call LLMs and
pay for them per request without exposing their prompts, their identity or their
spending strategy.

One key. An OpenAI-compatible endpoint. No SDK, no wallet, no integration work.

---

## The problem: inference leaks in three places

1. **Your data.** On a hosted provider your prompts run on servers you cannot
   inspect. You do not know how long they are kept, whether they train the next
   model, or whether you were served the model you paid for.
2. **Your identity.** An API key means an account, KYC, a card on file and a
   billing history. The provider knows who you are, what you use and how often.
3. **Your strategy.** x402 lets agents pay per request over HTTP with no
   account, but blockchains are public. A trading bot, a research agent or a
   fleet of agents publishes every payment: who it pays, how much and how often.
   Anyone watching can rebuild the strategy. Paying on chain for every call also
   costs gas and time that an agent making thousands of calls cannot afford.

## What Veil402 does about each one

| Leak | Layer | How |
|---|---|---|
| Data | **Private, verifiable compute** | Requests run on 0G's private compute inside TEEs. The provider cannot read the prompt, and 0G's attestation proves the work ran in that hardware. |
| Identity | **A VPN-style routing layer** | A local daemon, your own key, an OpenAI-compatible endpoint. Requests pass through a gateway in an AWS Nitro enclave, and funds can only be locked against an enclave key registered on chain to a measured image. Responses come back encrypted to you. No account, and the provider never learns who asked. |
| Strategy | **Shielded payment channels** | Funds sit as private notes in a zero-knowledge pool on Hedera. A session locks one note as a spending ceiling, is metered off chain, and settles once — paying the seller a new private note. |

**The result:** thousands of agent calls become one on-chain event, and that
event reveals neither the buyer, the amount spent nor the session's budget.

---

## How it works

```
 your tool (Aider, OpenHands, Langflow, any OpenAI client)
      │  http://127.0.0.1:8402/v1   + sk-veil-… key
      ▼
 ┌──────────────┐  HPKE-sealed    ┌──────────────────────────┐  TLS terminates   ┌──────────┐
 │ veil402      │ ───────────────▶│ gateway  ──vsock──▶ meter │ ─inside enclave──▶│ 0G TEE   │
 │ daemon       │◀─────────────── │ (host)      (Nitro enclave)│◀──────────────────│ provider │
 │ (buyer)      │  sealed answer  └──────────────────────────┘                   └──────────┘
 └──────┬───────┘                              │ one signature per session
        │ shield · lock · settle               ▼
        └──────────▶ relay ──────────▶  Pool.sol on Hedera  (Groth16 verifier, Poseidon tree)
```

1. **Shield once.** The daemon deposits tokens into the pool. They become a note
   only you can open.
2. **Open a channel.** For a session the daemon derives a fresh session key,
   moves value into a session note and **locks** it against a seller with a
   deadline. The enclave co-signs the lock, agreeing to meter it.
3. **Call as much as you like.** Each prompt is sealed to the enclave's attested
   key. The host relays ciphertext it cannot read; the enclave calls 0G, checks
   the provider's TEE signature, meters the call and seals the answer back.
   No transaction, no gas, no block wait per call.
4. **Settle once.** The enclave signs the metered total and a Merkle root over
   every call. One `settleToNote` pays the seller a shielded note and returns
   the unspent balance to you as a private change note. If the seller never
   settles, the deadline passes and the note is yours to spend again.

---

## The cryptography

The privacy is not a policy. It comes from a small number of constructions, each
doing one job, that compose into a payment channel nobody can observe.

### 1. Notes: value that is only a hash

A note is a leaf in a depth-20 Poseidon Merkle tree on chain:

```
owner       = Poseidon(3, Poseidon(1, spending_key), Poseidon(2, viewing_key))
note_key    = Poseidon(4, owner, random)
commitment  = Poseidon(5, note_key, asset_id, value)
nullifier   = Poseidon(6, Poseidon(2, viewing_key), leaf_index)
```

The chain stores only `commitment`. It is hiding (the `random` blinding means two
notes for the same owner and amount look unrelated) and binding (it opens to
exactly one owner, asset and value). Spending publishes the `nullifier`, which
only the viewing key can compute, so a note cannot be spent twice and nobody
else can tell which leaf it came from. Every hash carries a domain tag, so a
value produced in one role can never be replayed in another.

Spending authority and viewing authority are separate keys: you can let a
service *see* notes without letting it *spend* them.

### 2. The join-split: one proof, no graph

Every move after a deposit is a **2-in / 2-out join-split** — a Noir circuit
proved with Groth16 over BN254 (gnark), verified on chain. It proves, without
revealing any of them:

- both input notes are in the tree under a known root,
- their nullifiers are correctly derived,
- every value fits in 64 bits,
- inputs plus the signed public amount equal outputs,
- the proof is bound to this exact pool, chain, token, verifier, recipient and
  output ciphertexts (`ext_data_hash`).

The chain sees two nullifiers and two new commitments. It never sees which
deposit funded which spend, or how much moved. Fixed arity hides the shape:
zero-valued dummies pad a one-input payment to 2×2, so a payment, a merge and a
change-split are indistinguishable. Binding the external data into the proof
means a relayer that broadcasts it cannot redirect, re-price or swap the
ciphertexts.

### 3. Escrow without moving money

`lock` does not transfer anything. It **reserves a nullifier** against a seller
until a deadline, so the note cannot be spent anywhere else while the session
runs. The pool never holds an escrow balance and never learns the ceiling —
only that some note is reserved.

- The lock is EIP-712 signed by an enclave the `EnclaveRegistry` says is live,
  so every reservation is attributable to a key buyers pin and the operator can
  revoke.
- The seller can settle up to the deadline; the buyer can spend the note again
  only after a 60-second grace. The two can never race for the same instant.
- Locks are capped at one hour, so nobody can strand a note.

### 4. Settlement: one signature for a thousand calls

Every metered call is reduced to an attestation digest over the request, the
response and 0G's TEE signature. The enclave folds them into a **calls root** — a
sorted-pair keccak Merkle tree, the same shape OpenZeppelin's `MerkleProof`
checks — and signs:

```
NoteSettlement(nullifier, sellerCommitment, callsRoot)    // EIP-712
```

`settleToNote` accepts a join-split that spends the escrowed note into a seller
note and a change note, but only alongside that signature. Nothing public moves,
so no amount is published. The buyer can later prove any single call was among
those charged, with an inclusion proof against `callsRoot`.

0G's signature covers the request and response hashes but nothing of ours, so an
identical prompt in another session would yield a replayable signature. Binding
each call to its escrow is what stops that.

### 5. Unlinkable sessions and minimal delegation

Each session note is owned by keys derived from a fresh seed
(`Poseidon(8|9|10, seed)` for spend, view and blinding). Two sessions from the
same agent share nothing on chain. The handover the enclave receives carries
that seed, the note's Merkle path and the session terms: it opens one note and
reaches nothing else in the wallet.

### 6. A gateway that cannot read you

- Prompts and answers are sealed with **HPKE (RFC 9180)** to the enclave's key.
  The seller's 402 challenge names the enclave's PCR measurements, and
  `EnclaveRegistry` binds each enclave signing key to its image's `PCR0`
  (attestation checked off chain against the AWS Nitro root). The pool refuses
  a lock no live, registered enclave has signed.
- The enclave's seed is never configured anywhere. It is derived from AWS KMS
  against a point fixed by the image, the same on every boot and present nowhere
  at rest. A boot that derives a different identity refuses to serve.
- Outbound TLS terminates inside the enclave. The host relays bytes it can
  neither read nor alter.

### 7. A relay so your address never signs

Every pool call except `shield` is authorised by a proof or an enclave signature,
not by who sends it. So locks, settlements and payments are broadcast by a
separate relay key. An address that signed both a lock and its settlement would
link them; the relay removes that link. The daemon refuses to start if a relay
is configured and does not answer — silently falling back would quietly stop it
being private.

### 8. Agent-to-agent payments and selective disclosure

- **Payment addresses.** `veil402:pay:v1:…` encodes a payee's owner identity and
  viewing key. A payer creates a note for that owner and seals its opening to
  the viewing key; the payee's daemon discovers it by scanning commitments. On
  chain: two nullifiers, two commitments, no sender, no recipient, no amount.
- **Proof of payment for x402.** The note's blinding, known only to payer and
  payee, is the receipt. A chain-watcher who saw the commitment cannot produce
  it, so cannot claim someone else's payment.
- **Disclosure to one auditor.** Opening one commitment proves exactly one
  payment and nothing about the change beside it or any other note — unlike a
  viewing key, which reveals everything, forever. The package is signed to a
  named verifier with a nonce, so it cannot be passed on.

The circuit is walked through constraint by constraint in
[`circuits/README.md`](circuits/README.md), and the pool, escrow and settlement
rules in [`contracts/README.md`](contracts/README.md).

### What the chain sees

| Action | Public | Hidden |
|---|---|---|
| `shield` | depositor, amount | who the note belongs to |
| `transact` | two nullifiers, two commitments | sender, recipient, amount, which deposit funded it |
| `lock` | a nullifier, the seller, a deadline | buyer, ceiling |
| `settleToNote` | that a session settled, its calls root | buyer, amount paid, change, call count |

---

## Repository

```
circuits/transaction/     Noir 2-in/2-out join-split, BN254, depth 20
prover/veil402-gnark/     gnark Groth16 worker, pinned transaction-v4 artifacts,
                          Solidity verifier and calldata exporters
contracts/                Foundry: Pool, EnclaveRegistry, generated Verifier
crates/veil402-sdk/       notes, witness, proving, sessions, EVM bindings
crates/veil402-daemon/    buyer: wallet, index, OpenAI-compatible agent API, console API
crates/veil402-gateway/   seller: routes, escrow, settlement, enclave link
crates/veil402-enclave/   the meter that runs inside Nitro
crates/veil402-relay/     broadcasts pool calls under a key that is not yours
web/                      landing page and operator console (React + Vite)
playground/               Langflow wired to the local stack
demo/                     two agents paying each other privately over x402
specs/                    design notes per feature
```

Built on audited parts wherever one exists: `poseidon-solidity`, `@zk-kit/lazy-imt`,
OpenZeppelin, `alloy`, `hpke`, `borsh`.

## Run it

Prerequisites: Rust, Go, Node 18+, Foundry, Nargo.

```bash
cp .env.example .env                      # fill in RPC, pool, token, keys
(cd prover/veil402-gnark && go build -o veil402-gnark .)
cargo build --workspace
make test

./target/debug/veil402-daemon keygen      # an owner for .env
./target/debug/veil402-gateway &          # seller, :8401
./target/debug/veil402-daemon &           # buyer, :8402
./target/debug/veil402-daemon deposit 5000000
./target/debug/veil402-daemon key new my-agent --budget 500000
```

Then point any OpenAI-compatible tool at it:

```bash
export OPENAI_BASE_URL=http://127.0.0.1:8402/v1
export OPENAI_API_KEY=sk-veil-…
aider --model openai/<model>
```

- Operator console: `cd web && npm install && npm run dev` — see [web/README.md](web/README.md)
- Langflow: [playground/README.md](playground/README.md)
- Agent-to-agent private payments: [demo/README.md](demo/README.md)

## Hedera testnet deployment

| Contract | Address |
|---|---|
| Pool | `0xeEF91fE38Fa1a55B139eabCb673b90A9572b66e1` |
| Verifier | `0x42532488f967329e2B090dCf68396Ef0dd63de69` |
| EnclaveRegistry | `0xed8Afa86b16f17C22E8a73359ef93BbaC6Ad01DA` |
| PoolToken | `0x7118aEAdd435700ee91544b81cCd8CeA678eF9AB` |

Measured gas: proof verification ~379k, `transact` ~997k, `lock` ~85k, settlement
~1.01M. Full record in [`contracts/deployments/296-testnet.json`](contracts/deployments/296-testnet.json).

The pool is immutable: no proxy, no admin, no upgrade path.
