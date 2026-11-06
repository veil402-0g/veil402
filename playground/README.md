# Playground — Langflow against the local stack

Langflow pointed at the daemon's agent API, so a flow's model calls are metered
against an agent key and settle through the shielded pool. Nothing here is part
of the product; it exists to prove the gateway works with an off-the-shelf agent
builder, the same way `aider` did.

```
Langflow :7860  ──>  daemon :8402 /v1  ──>  gateway :8401  ──>  0G router
   flow                buyer                   seller            upstream
```

## Run it

The Rust side first — both binaries read the repo's `.env`, so start them from
the repo root:

```bash
cargo build --workspace
./target/debug/veil402-gateway &      # :8401
./target/debug/veil402-daemon &       # :8402, ~20s before /healthz answers
./target/debug/veil402-daemon key new langflow --budget 500000
```

The key prints once. Put it in `playground/.env` (gitignored) beside the base
URL:

```
OPENAI_COMPATIBLE_BASE_URL=http://127.0.0.1:8402/v1
OPENAI_COMPATIBLE_API_KEY=sk-veil-…
LANGFLOW_SKIP_AUTH_AUTO_LOGIN=true
```

Then:

```bash
cd playground
uv venv --python 3.12 .venv
VIRTUAL_ENV=.venv uv pip install langflow
./run.sh                              # :7860, ~30s on first boot
```

In Langflow: **Model providers → OpenAI Compatible**, enter the same base URL
and key, Save. It fetches `/v1/models` live and lists what the daemon serves.
See finding 3 for why the env vars alone are not enough.

The console's **Playground** button opens `:7860`; override with
`VITE_PLAYGROUND_URL` in `web/`.

## Verified

| | |
|---|---|
| Models discovered by Langflow | `0gm-1.0-35b-a3b`, `0gm-1.0-35b-a3b-sia`, `glm-5.3` |
| Flow run via `POST /api/v1/run/<flow>` | `17 times 23 equals 391.` |
| Metered on the key | 1422 → 2995 units across the agent's turns |
| First call in an envelope | ~15–22s (proof, lock, indexing) |
| Later calls in the same envelope | ~2–4s |
| Tool calling | works — `finish_reason: tool_calls` |

## Findings

**1. The upstream must be 0G's router, not any OpenAI-compatible endpoint.**
`veil402-enclave/src/adapters/router.rs:383` parses `/models` expecting `type`,
`pricing_usd.{prompt,completion}`, `max_completion_tokens` and `verifiability`.
A standard `/v1/models` returns none of those, so every model filters out, the
catalogue is empty and nothing can be priced. Ollama or OpenAI as `VEIL_0G_BASE`
would need a shim that synthesises the 0G schema. `VEIL_0G_BASE` and
`VEIL_0G_KEY` are both `required()`, so the gateway will not boot without them.

**2. Langflow's plain OpenAI component cannot reach our models.** Its model list
is a hardcoded set of OpenAI's own names. Use the **OpenAI Compatible** provider
(`lfx_openai_compatible`, bundled in 1.13), which discovers live from
`{base}/models`. Its SSRF guard does allow loopback.

**3. Langflow seeds only an allowlist of credentials from the environment.**
`OPENAI_API_KEY` is imported on first boot; `OPENAI_COMPATIBLE_BASE_URL` and
`OPENAI_COMPATIBLE_API_KEY` are not, and `get_provider_variable_value` resolves
nothing outside a request context. The provider has to be saved through the UI
once — after that it lives in Langflow's own database.

**4. Do not hand-write Langflow's `variable` table.** Non-secret variables are
stored plain and secrets Fernet-encrypted. Encrypting both produced
`Connector URL must be an http(s) URL …; got 'gAAAAABq6…'` — the base URL was
read back without decryption. Deleting the rows and using the UI fixed it.

**5. `/api/v1/run/<flow>` needs `LANGFLOW_SKIP_AUTH_AUTO_LOGIN=true`.** Since
1.5, auto-login alone is refused for API calls. Acceptable on a loopback
instance; it is not a setting to carry anywhere else.

**6. A transient RPC failure wedges the gateway permanently.** After one
`could not read what is due … error sending request` against the QuikNode
endpoint, every later call hung with no response and no error — including plain
calls that had worked minutes before. Both RPCs answered fine when probed
directly, so the endpoint had recovered and the gateway had not. Restarting both
binaries cleared it. Worth a look: a blip on `VEIL_RPC_URL` should degrade to a
refused call, not a hang.

**7. `VEIL_LOGS_URL` and `VEIL_RPC_URL` fail independently.** Sync kept
advancing on hashio throughout finding 6 while calls were dead on QuikNode, so a
healthy-looking sync log says nothing about whether calls can be served.
