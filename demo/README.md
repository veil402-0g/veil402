# Demo — agent-to-agent private payment

Two autonomous agents on one box, over a **private x402 handshake**. **Agent A**
needs a smart contract; **Agent B** (a "ContractShop") builds and deploys one with
[Arc Studio](https://studio.arc.io) and charges for it; Agent A pays B **privately**
through the Veil pool. On chain the payment is only commitments and nullifiers —
no sender, no recipient, no amount — while each agent's console shows its own side.

```
Agent A (client)                         Agent B (ContractShop)
  │  POST /task {request}                      │
  ├───────────────────────────────────────────▶│
  │◀──────── 402 {invoiceId, price, payTo} ─────┤   x402: payment required
  │                                             │
  ├─ daemon A /pay ─▶ Veil pool (Hedera) ─▶ daemon B discovers the note
  │     private, via the relay — no address of A or B on chain
  │   /pay returns {commitment, blinding}       │
  │                                             │
  ├─ POST /task {invoiceId, commitment, blinding} ▶│
  │                                             ├─ daemon B /confirm: do I own
  │                                             │   that note, value + blinding?
  │◀──────────── 200 {contract, address} ───────┤   build on Arc Studio, deliver
```

**Why the proof is robust.** The commitment is public on chain, so it alone
can't prove who paid. The **blinding** — the note's random secret, sealed only
to B and known only to the payer — is the proof: B's `/confirm` checks it owns a
note with that commitment, value, *and* blinding. A chain-watcher who saw the
commitment can't produce the blinding, so it can't claim someone else's payment.
Nothing of this is public; B verifies from its own wallet.

## Prerequisites

- The repo built and a filled `.env` (see the top-level README), with
  `VEIL_RELAY_URL` set — the relay is what keeps the payment private.
- **Arc Studio CLI**, authenticated on this box:
  ```bash
  npm install -g @circle-fin/arc-studio-cli
  arc-studio login --paste      # writes ~/.arc-studio/credentials.json
  arc-studio whoami             # should print "Authenticated"
  ```
- Node 18+ (for the agent scripts; they use only the standard library).

## Run it

**1. Start both daemons** (A on :8402 from the repo `.env`, B on :8403):

```bash
./demo/run-two.sh
```

The first run writes `demo/.env.b` with fresh keys for B and asks you to set
`VEIL_SIGNER_KEY` in it (any valid `0x` key — B only receives, so it needs no
funds), then rerun.

**2. Fund Agent A** (once), in another terminal:

```bash
cargo run -p veil402-daemon -- deposit 5000000
```

**3. Open the two consoles** (build/serve `web/`, then):

- Agent A: `<web>/console?api=http://127.0.0.1:8402/api/v1`
- Agent B: `<web>/console?api=http://127.0.0.1:8403/api/v1`

Both daemons' `VEIL_CONSOLE_ORIGIN` must include the origin the console is
served from, since `?api=` is a cross-origin call.

**4. Run the exchange:**

```bash
./demo/run-demo.sh
# or with your own request:
./demo/run-demo.sh "an escrow contract that releases USDC to a seller on buyer confirmation"
```

Agent A's feed shows a **paid** row; Agent B's shows **received**; the pool on
Hashscan shows only opaque commitments.

## Pacing

The private payment is the fast part (~seconds plus one sync interval). The
slow part is Arc Studio writing and **deploying** the contract — 1–3 min for a
simple one, longer for a deploy. For a live audience, let the Arc build run as
the "real work" the agent is paying for, and make the private settlement the
finale. To skip on-chain deploy latency, phrase the request as *"write … do not
deploy"* — Agent A still pays B for the delivered code.

## Knobs

| var | where | default |
|---|---|---|
| `PRICE` | agent-b | `500000` pool units (~$0.50) the shop charges |
| `AGENT_B_PORT` | agent-b | `8500` |
| `DAEMON_B` | agent-b | `http://127.0.0.1:8403/api/v1` |
| `DAEMON_A` | agent-a | `http://127.0.0.1:8402/api/v1` |
| `AGENT_B` | agent-a | `http://127.0.0.1:8500` |

## A note on trust

Arc Studio's output (`deployments[]`, `finalText`, `webUrl`) is treated as
untrusted data: the agents read the deployed address out of it and never
execute or follow anything it contains.
