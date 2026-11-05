# 003 — Escrow Envelope and Metered Sessions

## Overview

Introduces the payment envelope — a locked note that funds one inference session. The envelope reserves tokens from the shielded pool, meters usage call-by-call, and settles the remainder back when the session ends. A daemon manages envelope lifecycle and a persistent commitment tree index.

## Envelope Lifecycle

```
lock(note, recipient, deadline)
  │
  ├── session_open(envelope_id, session_key)
  │       │
  │       ├── call(prompt_tokens, completion_tokens)  ← repeats
  │       │
  │       └── settle(used, change_commitment)
  │
  └── expire(deadline)  → change returned to locker
```

### Lock

A note owner locks a portion of their balance into an envelope. The lock records:
- The backing note's nullifier (prevents double-lock)
- A recipient (the inference provider)
- A deadline (block height after which the locker can reclaim)
- The locked amount

### Metered Session

Each envelope supports one active session. The session holds:
- Its own note owner (derived per-session so sessions are unlinkable)
- A running total of tokens consumed
- A reference to the enclave key that the registry vouches for

Calls accumulate usage. The provider bills per-call using a scale and margin, not a single multiplier.

### Settlement

When the session ends, the enclave proves the metered total and produces:
- A settlement proof (total consumed, signed by the enclave)
- A change commitment (remaining balance → new note for the original owner)

If the envelope expires before settlement, the full balance returns to the locker via the daemon's recovery path.

## Commitment Tree Index

The pool's Merkle tree is stored incrementally rather than rebuilt from genesis on every restart. The daemon runs a persistent indexer that:

1. Watches on-chain events for new leaf insertions
2. Appends leaves to a local depth-20 tree
3. Persists the tree state (filled subtrees + next index) to disk
4. Serves Merkle paths to the SDK on demand

## Enclave Key Derivation

Session keys are derived deterministically from a root seed:

```
enclave_key = HKDF(root_seed, "veil402/enclave" || session_id)
```

The registry attests the derived public key, and the session's authority is sealed to that key — only the enclave holding the root seed can produce valid session signatures.

## Call Tracking

Rather than blocking on each settlement transaction, the daemon publishes what a settlement paid and tracks the on-chain call asynchronously. This lets the session continue serving while prior settlements confirm.
