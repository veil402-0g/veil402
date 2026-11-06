# 006 — Agent-to-agent private payments

## Intent

Show one agent paying another agent, privately, and prove the privacy on
chain. Today the pool only moves value between a wallet and itself (change,
settlement) or out to a public seller. This adds the missing edge: a direct
shielded transfer from one independent wallet to another, discovered by the
payee without either address appearing on chain. Two autonomous agents run the
exchange — one performs a task, the other pays it — and two operator consoles,
side by side, show each side's truth while the chain in between shows nothing.

The point being demonstrated: control and settlement without disclosure. An
observer watching the pool sees two commitments and two nullifiers and learns
neither the sender, the recipient, nor the amount; each agent's console shows
exactly what it is owed or has spent, and no more.

## Why now

The user has one funded daemon running on an EC2 instance and wants a
presentable demo of private agent-to-agent payment on that same box. The
cryptographic machinery for the receiving side already exists and runs; only the
sending path, an address format, a second daemon, and the two agents are new.

## Goals

- A wallet can pay an arbitrary recipient address a chosen amount, as one
  shielded `transact`, broadcast through the relay so no buyer address appears.
- The recipient discovers the incoming note automatically and can spend it.
- A **payment address** both agents can exchange: recipient owner id + viewing
  public key, encoded as one copy-pasteable string.
- Two daemons (Agent A, Agent B) run on one EC2 instance against the already
  deployed pool and relay, each with its own console tab.
- Two autonomous agents: Agent A requests a task, Agent B performs it and
  returns a price and its address, Agent A pays privately, Agent B sees it
  arrive — end to end with no human step beyond starting the run.
- The on-chain transaction is shown to reveal nothing (a Hashscan link to the
  opaque `transact`).

## Non-goals

- No escrow, enclave, or metering on this path — a direct transfer needs none of
  it. (The existing buyer→seller metered path is untouched.)
- No new circuit, contract, or trusted setup — this is an ordinary 2-in/2-out
  `transact` the deployed verifier already accepts.
- No note-merging: a payment spends a single input note, so one held note must
  cover the amount. Multi-note payments are later work.
- No `settleToNote`-style private-amount settlement — that is a different path.
- Not a production custody story. Both daemons run on one box the user controls;
  this demonstrates the protocol, not an operational deployment.

## Anti-goals (how this fails even if it runs)

- The payee's address or amount becoming inferable on chain. If an observer can
  link sender→recipient or read the amount, the demo is pointless.
- The payment appearing to work while actually moving value inside one wallet
  (the thing we established a single daemon cannot do). It must be two
  independent owners and a real on-chain `transact`.
- A demo that depends on fragile external liveness (0G router, a live gateway)
  for the payment itself. The payment must stand on its own.

## Background — what already exists (verified in-tree)

- **Addressed, sealed output.** `Send { owner, value, random, encrypted }`
  (`sdk/protocol/note.rs`) carries an output note to any recipient owner id with
  an encrypted payload. `wallet::output(asset, value, owner, random, seed, view)`
  already builds `(Minted, Send)` for an arbitrary `owner` and an arbitrary
  `view` key — it is only ever called today with the wallet's own.
- **Sealing to a recipient.** `seal_note(view, …)` HPKE-seals to any X25519
  public key (`sdk/session/key.rs`). The wallet exposes its own key via
  `WalletService::view()`.
- **Discovery is already a running loop.** Every `VEIL_INTERVAL` (default 5s)
  the daemon runs `wallet::discover()` → `reconcile()` (`daemon/app/mod.rs`),
  which scans new commitments, opens any sealed to this viewing key, verifies the
  opening reproduces the commitment, and writes the note down. The non-session
  branch of `opened()` already handles "a plain note someone paid me," owned by
  this wallet's key and therefore spendable. **No change needed to receive.**
- **Spend plumbing.** `Veil::transfer(transaction)` proves and encodes a
  transfer; the daemon's prove adapter, relay adapter, and index (for Merkle
  paths) are already wired — the `reseed` → `prover.transfer` → `relay.submit`
  sequence in `daemon/core/envelope/service.rs` is the template to copy.
- **Config is env-driven.** `VEIL_LISTEN`, `VEIL_WALLET_STATE`, `VEIL_DATABASE`,
  `VEIL_SPEND_KEY`/`VEIL_VIEW_KEY` are per-process; `VEIL_POOL`, `VEIL_TOKEN`,
  `VEIL_VERIFIER`, `VEIL_RPC_URL`, `VEIL_RELAY_URL` are shared. The web console
  reads its daemon from `VITE_API_BASE` and `client(base)` already takes a base.

## Design

### 1. Payment address

A recipient's address is the pair already derivable from its owner keys:

- `owner` = `Owner::public()` (Field, 32 bytes) — sets the output note's npk.
- `view` = `Viewing::new(&owner.view).public()` (X25519, 32 bytes) — what the
  sender seals to.

Encoding (borsh, per the repo's canonical-serialisation rule, with a scheme
prefix and base64url): `veil402:pay:v1:<base64url(borsh{owner,view})>`. Both
halves are public; the string is safe to copy, paste, and show on screen. A
`PaymentAddress` type with `to_string`/`from_str` lives in the SDK beside the
other session/address types.

The daemon exposes its own address at `GET /console/address`. `WalletService`
gains `identity() -> Field` (`self.owner.public()`) to supply the owner half;
`view()` already supplies the other.

### 2. The pay path

New `WalletService::pay(recipient: PaymentAddress, amount, input: &Held, merkle)
-> Prepared`-style result, mirroring `reseed`:

1. Require `input.value >= amount` (else `WalletError::Empty` / a "no single note
   covers this amount" error).
2. Recipient output: `output(asset, amount, recipient.owner, &fresh, None,
   recipient.view)` → keep only the `Send` (the `Minted` is the payee's, not
   ours). This seals the opening to the recipient's viewing key, so only they
   discover it.
3. Change output: `output(asset, input.value - amount, self.identity(), &fresh,
   None, self.view())` → keep both; `remember` the change `Minted` **before**
   the transaction is sent (same durability rule as every other mint). If change
   is zero, emit only the recipient output; planning pads the second slot.
4. Input `Spend { note, owner: owner_for(input), merkle }`.
5. `Transaction::new(vec![input], vec![recipient_send, change_send])`.

App layer (a `pay` use case next to the deposit/reseed ones): reserve a
spendable note, fetch its Merkle path from the index (same call `reseed` uses),
call `wallet.pay(...)`, `prover.transfer(...)`, `relay.submit(...)`, then track
landing/placement. Returns the tx hash and the recipient + change commitments.

Because it goes through the relay, the on-chain `transact` is signed by the
relay key, not either agent — no buyer/payee address appears.

### 3. Receiving — unchanged

Agent B's running `discover()` loop picks up the note within one interval
(~5s), opens it with B's viewing key, confirms the commitment, and writes it as
a spendable note. Its balance rises; it owns the note outright.

### 4. Console

Receiving a note makes it spendable on its own (the discovery loop writes it
down), but **showing** it on a console is not free. `console/activity.rs`
assembles the feed from three fixed sources — settled calls, escrows, and the
last deposit — with `Kind = Deposit | Escrow | Settled`; nothing lists sent or
received notes, and `Minted`/`Held` carry no receipt timestamp. So the visible
demo needs real plumbing on both sides, not just a render:

- `GET /console/address` — this daemon's payment address string (+ optional QR).
- `POST /console/pay { address, amount }` — runs the pay use case; returns the tx
  hash and commitments, and records a **Paid** event (amount, recipient
  commitment, tx hash, time) on Agent A.
- A **Received** event on Agent B: when `discover()` writes down a note the
  wallet had no record of, append a lightweight receipt (amount, commitment,
  time). `discover()` finds exactly these — notes owned but unrecorded — and own
  change is written by the pay/settle flows directly, never via discovery, so a
  discovered note is unambiguously an incoming payment.
- New `Paid`/`Received` kinds in `console/activity.rs` and the web `EventKind`
  union + rendering; a "Pay" dialog (modelled on the deposit dialog) and a
  copyable "My address" field. Both consoles already page/sort their feeds.

### 5. Two autonomous agents

Two small scripts (one per agent), **scripted rather than Langflow** by default
— see Decisions. Each sits beside its daemon:

- **Agent B (worker/payee).** A tiny HTTP service: `POST /task {input}` →
  performs the task, fetches its own `GET /console/address` from daemon B,
  returns `{ result, price, address }`.
- **Agent A (orchestrator/payer).** On a start trigger: send a task to B,
  receive `{result, price, address}`, call daemon A `POST /console/pay
  {address, amount: price}`, print the tx hash.

"Autonomous" = the whole exchange (task → result → price → payment → receipt)
runs from one trigger with no human steps in between.

Agent B answers A's task with a real, minimal inference through its daemon's
`/v1` → gateway → 0G path (the existing buyer-agent flow), then charges A for it.
See the Resolved section; the direct-call fallback is a one-line switch in B's
script.

### 6. Two daemons on one EC2

Agent A is the already-running funded daemon (payer). Agent B is a second
process:

- Own `VEIL_SPEND_KEY`/`VEIL_VIEW_KEY` (from `keygen`), own `VEIL_WALLET_STATE`
  and `VEIL_DATABASE`, own `VEIL_LISTEN` (e.g. `127.0.0.1:8403`).
- Shared `VEIL_POOL`/`VEIL_TOKEN`/`VEIL_VERIFIER`/`VEIL_RPC_URL`/`VEIL_RELAY_URL`.
- `VEIL_GATEWAY_URL` must be set (the daemon requires it to start) — both point
  at the same gateway URL; the pay path never calls it, and the catalogue is
  read lazily, so it need not be live for payments.
- Agent B still needs `VEIL_SIGNER_KEY`, `VEIL_SELLER`, `VEIL_WORKER`, and
  `VEIL_ARTIFACTS` set, with the gnark worker and artifacts present on disk —
  the daemon requires them to boot even though B only receives and never proves
  or buys. The worker binary and `transaction-v4` artifacts can be the same
  files Agent A uses.
- Agent B needs no initial funds; it starts empty and receives. Agent A funds
  itself via the existing deposit flow if it is not already holding a note.

A `demo/` runner (`run-two.sh` or two compose services) starts both daemons and
opens the two console tabs — daemon A console at its `VITE_API_BASE`, daemon B
console pointed at `127.0.0.1:8403`.

### 7. The demo

Two console tabs side by side (A and B). The runner triggers the autonomous
exchange; Agent A's console shows the outgoing payment and its balance fall,
Agent B's shows the received note and its balance rise, within one sync
interval. The punchline is a Hashscan link to the `transact`: it shows two
opaque commitments and two nullifiers — no sender, no recipient, no amount.

## Decisions made (builder's / author's calls, overridable)

- **Scripted agents over Langflow.** The user accepted "Langflow or scripted."
  Two small scripts are reliable, controllable, and show autonomy without
  fighting Langflow's custom-component model for a non-OpenAI "pay" action.
  Langflow remains a later dressing for Agent A if wanted.
- **Two-hop live inference** (see Resolved): B is an ordinary buyer-agent and
  does a real minimal model call; A pays B privately for it.
- **Address encoding**: borsh + base64url with a `veil402:pay:v1:` prefix, for
  the same reasons borsh is used elsewhere (deterministic, rejects trailing
  bytes). No on-chain component, so the format is ours to choose.
- **Single-input payments only** for v1; a payment whose amount exceeds every
  single held note is refused with a clear error rather than silently merging.
- **Change sealed to self as a plain (non-session) note**, so `discover()`
  recovers it; it counts toward balance and is spendable by the next payment.
- **Second daemon on port 8403**, consoles in two browser tabs (the user's
  chosen demo surface).
- **Transaction-hash capture for the demo link.** The relay returns only `202`
  and the chain source currently drops `log.transaction_hash`, so nothing yet
  learns the `transact`'s hash. The chain source will capture
  `transaction_hash` alongside each `NewCommitment`, and the `Paid` event will
  carry it so the console can link the exact tx on Hashscan. Fallback if that is
  fiddly: a contract-level Hashscan link to the pool, which still shows the
  opaque commitments and nullifiers.
- **Two-console URL selection.** `web/src/api/client.ts` bakes `VITE_API_BASE`
  at build time, and `client(base)` takes an override but `App` has no runtime
  way to choose 8402 vs 8403. Add a runtime `?api=` query-param override that
  `App` reads and passes to `client(base)`, so one build serves both tabs. (A
  second build or a dev proxy would also work; the query param is least setup.)

## Resolved — two-hop live inference

**Agent B does real, minimal inference through the existing path.** The chosen
showcase is a fully private two-hop economy:

- **A → B** is the new private payment (the deliverable this spec adds).
- **B → model** is the existing metered inference the product already does: B is
  an ordinary Veil buyer-agent that answers A's task by calling a small model
  (e.g. `glm-5.3`, capped output) through its daemon → gateway → 0G, paying for
  it privately through the pool.

On Hashscan both hops show only opaque commitments and nullifiers. The only new
code is the A → B payment; B's side reuses the buyer-agent flow unchanged.
"Minimal compute" = a short prompt and a small model, so each call is cheap.

**Dependency this leans on:** the gateway + 0G path must be live and funded on
the EC2 box, and B's agent key needs a small budget. The core A → B payment path
is built and tested independently of this, so it is unaffected if the inference
path is down. **Fallback** (demo only): B makes a direct minimal model call
outside the pool, leaving A → B as the single private hop. The switch lives in
Agent B's script, not in the payment code.

## What's left to the builder

- Exact wire shapes of `/console/address` and `/console/pay` (follow the
  existing console DTO conventions).
- The agent scripts' language and the sample task content.
- The `demo/` runner's form (shell vs compose) and how the two console tabs are
  launched on the EC2 box (local vs port-forwarded for viewing).
- Console UI placement and styling of the Pay dialog, My-address field, and the
  received-payment activity entry, consistent with the existing console.
- Tests: a daemon-level test that a note sealed to another wallet's address is
  discovered and spendable by that wallet; a pay-path test that value is
  conserved and the change returns to the sender; that a `Paid` event on the
  sender and a `Received` event on the recipient appear on their feeds after one
  payment.
- Exact shape of the `Paid`/`Received` event records and where they persist (a
  small events log vs. extending the note store), and the web rendering for the
  two new kinds.

## Constraints & risks

- **Relay required** for the privacy claim; the daemon already refuses to start
  with `VEIL_RELAY_URL` set but unreachable, which is the correct failure.
- **Timing**: recipient console updates within one `VEIL_INTERVAL` (~5s) — fine
  for a live demo; worth narrating so the few-second gap reads as expected.
- **Trusted setup** remains the inherited open risk (no ceremony); this is a
  testnet demo, not real money, so it does not block.
- **Funds**: Agent A must hold a single note ≥ the payment amount; seed the
  deposit large enough for the demo's prices.
