#!/usr/bin/env bash
# Two daemons on one box: A on :8402 (the repo .env), B on :8403 (demo/.env.b).
# A is the payer (fund it with `cargo run -p veil402-daemon -- deposit <units>`);
# B starts empty and receives.
set -euo pipefail
cd "$(dirname "$0")/.."

[ -f .env ] || { echo "no repo .env — copy .env.example and fill it first"; exit 1; }

if [ ! -f demo/.env.b ]; then
  echo "demo/.env.b not found — creating it with fresh keys for B"
  cp demo/.env.b.example demo/.env.b
  cargo run -q -p veil402-daemon -- keygen >> demo/.env.b
  echo
  echo "wrote demo/.env.b with B's VEIL_SPEND_KEY / VEIL_VIEW_KEY."
  echo "now set VEIL_SIGNER_KEY in demo/.env.b (any valid 0x key, no funds needed), then rerun."
  exit 1
fi

cargo build -q -p veil402-daemon

echo "starting daemon A on :8402"
cargo run -q -p veil402-daemon > demo/daemon-a.log 2>&1 &
A=$!

echo "starting daemon B on :8403"
( set -a; . ./demo/.env.b; set +a; exec cargo run -q -p veil402-daemon ) > demo/daemon-b.log 2>&1 &
B=$!

trap 'kill "$A" "$B" 2>/dev/null || true' EXIT

health() {
  for _ in $(seq 1 120); do
    curl -fsS "http://127.0.0.1:$1/healthz" >/dev/null 2>&1 && return 0
    sleep 2
  done
  echo "daemon on :$1 never became healthy — see demo/daemon-$2.log"; return 1
}

echo "waiting for both to index the pool (~30s each)…"
health 8402 a
health 8403 b

echo
echo "both up."
echo "  Agent A console:  <web>/console?api=http://127.0.0.1:8402/api/v1"
echo "  Agent B console:  <web>/console?api=http://127.0.0.1:8403/api/v1"
echo
echo "now run ./demo/run-demo.sh to make Agent A buy a contract from Agent B and pay for it."
echo "logs: demo/daemon-a.log  demo/daemon-b.log   (Ctrl-C stops both daemons)"
wait
