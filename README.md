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

---

## x402 on Hedera: the payment flow

Veil402 is an **x402** payment rail for **AI agents** that settles on **Hedera
testnet** (chain id 296). The seller's endpoint is **x402-protected**: a request
without a live payment gets **HTTP 402 Payment Required** and a challenge saying
what to pay. The buyer's daemon pays and retries **autonomously, with no human
approval** — the human sets the spending rules once.

The scheme is **`escrow`**, not `exact`: one payment opens a channel that
covers a whole session of metered calls. We run **our own facilitator** — the
gateway's settlement adapter verifies the enclave's meter and submits the
settlement, and `crates/veil402-relay` broadcasts it.

```
agent ──POST /v1/chat/completions──▶ daemon (buyer)
                                      │ GET  /v1/catalogue                  discover models and prices
                                      │ POST /v1/sessions/{n}/chat ──▶ 402  challenge: scheme "escrow",
                                      │                                     pool, chain 296, seller, enclave PCRs
                                      │ POST /v1/sessions/agree             enclave co-signs the lock terms
                                      │ Pool.lock(...) on Hedera ◀── relay  note reserved, no tokens move
                                      │ POST /v1/sessions/{n}/chat ──▶ 200  metered off chain, sealed answer
                                      ▼  … thousands of calls, zero transactions …
                       gateway ──▶ Pool.settleToNote / settleToAddress on Hedera
                                   one transaction per session, signed by the enclave
```

**Spending controls**, enforced before any money moves:

- per-agent API keys (`veil402-daemon key new <label> --budget <units>`), stored
  only as a hash;
- a budget cap per key, a model allowlist per key, pause, resume and revoke from
  the operator console;
- an escrow ceiling per session: the locked note is the most a session can spend;
- a lock deadline of at most one hour, after which the note returns to the buyer.

**Agent-to-agent negotiation.** [`demo/`](demo/README.md) runs two autonomous
agents over a private x402 handshake: Agent B answers `402 {invoiceId, price,
payTo}`, Agent A pays B's `veil402:pay:v1` address through the shielded pool,
and B confirms the payment before delivering.

### Hedera testnet evidence

| Step | Hedera transaction ID | HashScan |
|---|---|---|
| Groth16 proof verified on chain | `0.0.4507444-1793101034-630151893` | [view](https://hashscan.io/testnet/transaction/1793101038.217963439) |
| Pool deployed | `0.0.4507444-1793179079-580803111` | [view](https://hashscan.io/testnet/transaction/1793179085.382657104) |
| `shield` — deposit into the pool | `0.0.4507444-1793179103-539459791` | [view](https://hashscan.io/testnet/transaction/1793179111.478524104) |
| `lock` — x402 escrow opened | `0.0.4507444-1793179112-645978943` | [view](https://hashscan.io/testnet/transaction/1793179120.598529104) |
| `settleToAddress` — session settled | `0.0.4507444-1793179127-374412857` | [view](https://hashscan.io/testnet/transaction/1793179135.539354104) |

| Contract | EVM address | Hedera ID |
|---|---|---|
| Pool | `0xeEF91fE38Fa1a55B139eabCb673b90A9572b66e1` | [`0.0.11206027`](https://hashscan.io/testnet/contract/0.0.11206027) |
| Verifier | `0x42532488f967329e2B090dCf68396Ef0dd63de69` | [`0.0.11191610`](https://hashscan.io/testnet/contract/0.0.11191610) |
| EnclaveRegistry | `0xed8Afa86b16f17C22E8a73359ef93BbaC6Ad01DA` | [`0.0.11206026`](https://hashscan.io/testnet/contract/0.0.11206026) |
| PoolToken | `0x7118aEAdd435700ee91544b81cCd8CeA678eF9AB` | [`0.0.11206025`](https://hashscan.io/testnet/contract/0.0.11206025) |

Measured gas: proof verification ~379k, `transact` ~997k, `lock` ~85k, settlement
~1.01M. Full record in [`contracts/deployments/296-testnet.json`](contracts/deployments/296-testnet.json).

The pool is immutable: no proxy, no admin, no upgrade path.

---

## Built on 0G Private Computer

Every inference Veil402 sells runs on **0G Private Computer**: the enclave calls
0G's OpenAI-compatible router (`router-api.0g.ai/v1`), and each request executes
inside a **TEE** on a 0G provider.

- **0G is in the main user flow.** Every metered call is a 0G Private Computer
  inference; there is no other upstream.
- **Request IDs are kept.** 0G's `request_id` is recorded with every call, hashed
  into that call's attestation, and rolled into the calls root the session
  settles against. The console's audit view (`GET /api/v1/audit`) shows it per
  call.
- **The TEE signature is checked.** The enclave recovers the provider's signature
  over the response and compares it with the TEE signer 0G publishes on chain.
- **Providers are filtered.** Only `TeeML`-verified nodes with an acknowledged
  signer are used.

**What breaks without 0G:** the escrow has nothing to release against. Without
0G's TEE attestation there is no proof the inference ran privately, so a session
could not be settled without trusting the seller.

---

## CROPS

Veil402's lead property is **Private**. The other three hold, and where they are
partial we say so.

### Control and dependencies

- You run the buyer daemon yourself, generate your own keys (`veil402-daemon
  keygen`) and prove locally. No account, no API key from anyone else.
- The pool is an immutable contract with no owner, proxy or pause. Nobody can
  freeze or seize a note.
- If a seller vanishes, the escrowed note returns to you after its deadline plus
  60 seconds. Withdrawing needs only your proof.
- The relay is a separate binary (`crates/veil402-relay`) you can run yourself.
- Dependencies we do not control: a Hedera JSON-RPC endpoint, the seller's
  gateway and Nitro enclave, AWS KMS on the seller's side, and 0G Private
  Computer. `EnclaveRegistry` has an owner who can register and revoke enclave
  keys. It cannot touch funds, and revoking a key does not strand a live escrow.

### License and buildability

- Apache-2.0 ([`LICENSE`](LICENSE)).
- Builds from source with `cargo build --workspace`, `go build` and `forge build`.
  Contract dependencies are pinned by commit (`make deps`), proving artifacts are
  pinned by version, and Foundry is pinned.
- No trackers or analytics in the web client. Its only third-party request is
  for typefaces from Google Fonts.

### What third parties learn

| Party | Learns | Does not learn |
|---|---|---|
| Chain observers | that some note was spent, locked or settled | buyer, seller's amount (`settleToNote`), balances, which deposit funded what |
| Relay operator | the transactions it broadcasts, and your IP | your keys, amounts, which notes are yours |
| Gateway host | ciphertext sizes and timing, and your IP | prompts or answers (HPKE-sealed to the enclave) |
| Enclave (attested image) | the prompt, to run it | who you are on chain |
| 0G provider | the prompt, inside its TEE | who sent it — the request comes from the enclave's account |
| Seller | what the session cost | your identity or wallet |
| JSON-RPC provider | your IP and the pool logs you sync | which notes are yours — the daemon reads every log, not yours alone |

Trade-off stated: network-level metadata (your IP) is visible to the RPC
provider, the relay and the gateway. Routing those through an anonymity network
is the next step.

### How keys and data are protected

- Spending and viewing keys are generated locally and never leave the daemon.
  Each session uses a fresh derived key, so sessions cannot be linked.
- Agent API keys are stored only as a hash and printed once.
- Proving runs locally. The prover worker never writes the witness to disk.
- Prompts and answers are sealed with HPKE (RFC 9180). Note openings are sealed
  to the recipient's viewing key.
- The enclave's key is derived from AWS KMS against its image measurement and is
  never stored anywhere.
- One payment can be disclosed to one auditor without revealing any other
  (`crates/veil402-sdk/src/protocol/disclosure.rs`).
