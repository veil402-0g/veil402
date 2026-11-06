# Veil402 — a VPN layer for private inference

Privacy for inference needs two layers. The industry has one.

**Hardware layer** — solved by 0G. The seller must never read your prompt, and you need proof the inference actually happened. 0G's TEE providers do this.

**Payer layer** — nobody has solved it. Today you create an account, generate an API key, and hand over your identity and billing.

## Veil402

Three things, one API key:

| | |
|---|---|
| **0G private inference** | TEE attestation for the compute. We don't replace it — we depend on it. |
| **Shielded payments** | A zero-knowledge pool. No deposit→spend link, no payment graph, no counterparty history. |
| **Escrow-based x402** | Funds move only when work is proven. HTTP 402 means no account, no API key, no identity at the provider. |

## Developer experience is the product

One key. OpenAI-compatible. Already running against Aider, OpenHands, Langflow — no SDK, no wallet, no integration work.
