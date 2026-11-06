# Veil402 — EVM

Shielded 2-in/2-out join-split pool, ported from the Solana implementation to EVM.
The circuit, prover and note protocol are unchanged; only the chain transport is new.

## Why the port is cheap

The circuit is Groth16 over **BN254** — the same curve the EVM's `ecPairing` precompile
speaks. The existing verifying key therefore produces a Solidity verifier directly, with
no new circuit and no new trusted setup.

## Layout

```
circuits/transaction/   Noir circuit — unchanged (2-in/2-out, depth 20)
prover/veil402-gnark/   gnark Groth16 worker + pinned transaction-v4 artifacts
  cmd/export-verifier     vk  -> Solidity verifier
  cmd/solidity-calldata   raw proof -> verifier argument layout
crates/veil402-sdk/     chain-agnostic client: notes, witness, proving
contracts/              Foundry: verifier, pool, escrow
docs/reference-spec/    design notes carried over from the Solana build
```

## Public inputs

Seven, in verifier order:

```
[ root, nullifiers[0], nullifiers[1], commitments[0], commitments[1],
  public_amount, ext_data_hash ]
```

The exported verifier also takes a gnark Pedersen commitment and its proof of knowledge:

```solidity
verifyProof(uint256[8] proof, uint256[2] commitments,
            uint256[2] commitmentPok, uint256[7] input)
```

## Phase 0 — reproduce

```sh
# 1. export the Solidity verifier from the pinned verifying key
cd prover/veil402-gnark
go run ./cmd/export-verifier \
  -vk artifacts/transaction-v4/transaction.vk \
  -out ../../contracts/src/Verifier.sol

# 2. produce a real proof and its public inputs
cd ../..
cargo test --manifest-path crates/veil402-sdk/Cargo.toml --test proving

# 3. convert the proof into verifier calldata
cd prover/veil402-gnark
go run ./cmd/solidity-calldata \
  -fixture ../../contracts/test/fixtures/transfer.json \
  -out ../../contracts/test/fixtures/calldata.json

# 4. verify it on a local EVM
cd ../../contracts && forge test -vv
```

## Running the stack

Two binaries, one `.env`. The daemon is the buyer: it holds the notes and
serves an OpenAI-shaped API to the agent. The gateway is the seller: it
meters each call, relays it to the upstream, and settles against the pool.

```bash
cp .env.example .env            # then fill in the unset values
make deps                       # contracts/lib is gitignored
cd prover/veil402-gnark && go build -o veil402-gnark . && cd -
cargo build --workspace
```

Before the first call the pool has to know the seller, and the wallet has
to hold something to spend:

```bash
cargo run -p veil402-daemon -- keygen        # VEIL_SPEND_KEY / VEIL_VIEW_KEY
cargo run -p veil402-daemon -- deposit 5000000
```

`VEIL_GATEWAY_SIGNER` must be registered in the pool's enclave registry, or
every lock is refused with `EnclaveNotLive()`.

Start both. The daemon reads the seller's catalogue lazily, on the first
request rather than at boot, so the order does not matter:

```bash
cargo run -p veil402-gateway    # :8401
cargo run -p veil402-daemon     # :8402
```

The daemon's first sync indexes the pool from `VEIL_POOL_BLOCK`, which takes
about half a minute before it answers on `/healthz`.

Then issue a key and point any OpenAI client at it:

```bash
cargo run -p veil402-daemon -- key new aider --budget 500000
```

```bash
aider --openai-api-base http://127.0.0.1:8402/v1 --openai-api-key sk-veil-... --model openai/glm-5.3
```

The key is printed once and stored only as a hash. `key list` shows what each
has spent; `key revoke` stops one authenticating.

### What a call does

The daemon opens an *envelope* — one note, locked to the seller for
`VEIL_ENVELOPE_WINDOW` seconds — and every call inside it is metered against
that note rather than settled on chain. When the envelope fills or its
deadline nears, the daemon settles, the change comes back as a new note, and
the next call opens a fresh envelope. A buyer that walks away is swept by the
seller instead, so the work is always paid for.

The first call in a session pays for the envelope: a Groth16 proof, a lock
transaction, and waiting for it to be indexed. Later calls in the same
envelope pay only for inference.

### Gotchas

`VEIL_POOL_BLOCK` matters. Left at 0 the first sync walks the chain from
genesis, which against a live network is minutes.

`VEIL_WORKER` and `VEIL_ARTIFACTS` must be absolute paths.

## Status

| Check | Result |
|---|---|
| Solidity verifier exported from `transaction-v4` vk | done |
| Noir Poseidon matches circomlib (`hash_2`, `hash_3`, `hash_4`) | done |
| SDK builds with no Solana dependency | done |
| Real proof generated (388 bytes) | done |
| Proof verifies on a local EVM | **done — 395,368 gas** |
| Tampered public input is rejected | done |

### Gotcha worth keeping

The exported verifier derives the Pedersen commitment's public input with
`keccak256`. gnark's prover defaults to a different hash-to-field, so the
worker pins both sides to keccak via `WithProverHashToFieldFunction` /
`WithVerifierHashToFieldFunction`. Without that the pairing check fails with
`ProofInvalid()` even though every point is valid.
