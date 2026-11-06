# 004 — Enclave Gateway and Sealed Inference

## Overview

The enclave is a sealed compute environment that runs inference and proves what it billed. A gateway relays traffic between callers and the enclave. The agent model gives each caller its own key, budget, and ledger. All prompts and answers are HPKE-sealed so the relay sees ciphertext only.

## Gateway Architecture

```
caller ──► gateway (relay) ──► enclave
  │              │                 │
  │  HPKE seal   │   forward       │  unseal, infer, seal
  │◄─────────────│◄────────────────│
                 │                 │
          sees ciphertext    holds root seed
```

The gateway is a stateless relay the daemon talks to. It:
- Accepts sealed requests from agents
- Forwards them to the enclave
- Returns sealed responses
- Cannot read prompts or answers (HPKE encrypted to the enclave's key)

## Sealed Request Flow

1. Agent seals the prompt with the enclave's attested public key (HPKE)
2. The request opens its own envelope (or uses an existing session)
3. The enclave unseals the prompt, runs inference via an upstream provider
4. The answer is sealed back to the agent's key
5. The relay streams ciphertext chunks — it can neither read nor truncate

## Streaming and Billing

Answers flow as sealed chunks. Each chunk is independently verifiable. Billing happens when the stream ends:

```
stream_start(session_id)
  │
  ├── chunk(ciphertext, token_count)  ← repeats
  │
  └── stream_end(total_tokens, bill)
```

The bill is bound to the answer the buyer received — if the stream is cut short, only delivered tokens are charged.

## Agent Model

Each agent has:
- A keypair (for HPKE sealing)
- A budget (maximum spend across sessions)
- A set of books (ledger of calls, costs, settlements)
- A mandate (which models it may use, what base URL to hit)

An agent points its base URL at the gateway and changes nothing else in its calling code — the gateway transparently handles sealing, metering, and settlement.

## Provider Verification

The enclave checks the upstream provider's response signature against the key its quote attested. This binds the bill to a specific provider's output, preventing a relay from substituting a cheaper model's output.

## Settlement Proving

Settlement can be initiated from outside by either party against a real chain:
- The buyer can settle its own envelope before the seller sweeps it
- The seller can close a session the buyer walked away from
- Both paths produce a valid settlement proof checkable on-chain

## Recovery

The daemon handles edge cases:
- Envelopes that expired unsettled → change returned to locker
- Restarted enclave → picks up existing sessions from persisted state
- Chain with existing history → both sides sync from the current state
