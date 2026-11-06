#!/usr/bin/env bash
# Langflow against the local Veil402 daemon. Start the gateway and daemon first.
set -euo pipefail
cd "$(dirname "$0")"
set -a; source .env; set +a
exec .venv/bin/langflow run --host "$LANGFLOW_HOST" --port "$LANGFLOW_PORT"
