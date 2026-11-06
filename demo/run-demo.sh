#!/usr/bin/env bash
# One end-to-end exchange: Agent A buys a smart contract from Agent B (built on
# Arc Studio) and pays for it privately.
#
# Prerequisites:
#   - ./demo/run-two.sh is already running (daemons A:8402 and B:8403)
#   - daemon A holds a funded note: cargo run -p veil402-daemon -- deposit 5000000
#   - Arc Studio is authenticated: arc-studio login --paste  (then: arc-studio whoami)
set -euo pipefail
cd "$(dirname "$0")"

command -v arc-studio >/dev/null 2>&1 || { echo "arc-studio not installed (npm i -g @circle-fin/arc-studio-cli)"; exit 1; }
arc-studio whoami >/dev/null 2>&1 || { echo "Arc Studio not authenticated — run: arc-studio login --paste"; exit 1; }
command -v node >/dev/null 2>&1 || { echo "node not found"; exit 1; }

echo "starting Agent B (the ContractShop)…"
node agent-b.mjs > agent-b.log 2>&1 &
B=$!
trap 'kill "$B" 2>/dev/null || true' EXIT

for _ in $(seq 1 30); do
  curl -fsS http://127.0.0.1:8500/address >/dev/null 2>&1 && break
  sleep 1
done

echo "running Agent A (the client)…"
echo
node agent-a.mjs "$@"

echo
echo "watch the two console tabs: Agent A's feed shows a 'paid' row, Agent B's shows 'received'."
echo "Agent B log: demo/agent-b.log"
