# 005 — Operator Console and Web Frontend

## Overview

A web frontend served alongside the daemon's agent API. The landing page introduces the protocol; the console gives operators real-time visibility into agents, mandates, escrows, and deposits.

## Landing Page

Static marketing page with:
- Protocol introduction and value proposition
- Poster section (visual assets from the design files)
- Playground link (Langflow integration for testing against the local stack)
- Favicons and vendor tiles (Hermes)

## Console

A live operator dashboard drawn from the daemon's own state:

### Agent Table
- Paginated, searchable, sortable list of agents
- Each row shows: name, status (styled badge), budget, usage
- Row actions: pause, revoke (in one cell), copy mandate key
- Agent names stay on one line in the range chart

### Mandate Management
- Create new mandates with generated keys
- Reveal key with a cover/uncover interaction (wipe to read)
- Key copied from the actions column, not from under the name
- Dialog clears the previous key when reopened

### Escrow View
- Shows only the escrows this daemon opened
- Reads the lock from the envelope, not from the draws on it
- Envelope past its deadline is not shown as the active one

### Deposit Flow
A multi-step deposit visualization:
```
initiate → confirm → on-chain → indexed
```
- Drawn as a single track with checkpoints (not three lanes)
- Gradient fill shows progress
- Running steps sweep rather than hold a fixed figure
- Fill caps past the checkpoint
- Tick marks drawn as SVG, not typed characters
- Outcome shown on the button, not under the track

### Live Updates
- Actions read back their result immediately
- Controls are either functional or explicitly marked as not yet wired
- Escrow and deposit state refreshes from the daemon

## Additional Features

### Attestation Fallback
When the serving contract is unavailable, the system serves without attestation rather than failing entirely.

### Note Discovery
Wallet can find its own notes on chain by scanning commitment events against known viewing keys.

### Relay Broadcasting
Locks and settlements are broadcast under a key that is not the daemon's own, supporting multi-operator topologies.
