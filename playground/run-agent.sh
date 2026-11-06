#!/usr/bin/env bash
# Langflow with the Veil402 "Commission Contract" component loaded.
#
# In Langflow, build a flow:  Chat Input -> Commission Contract -> Chat Output
# then open the Playground and ask for a contract. Agent B (the ContractShop)
# and daemon A must already be running:
#   STUB=1 node demo/agent-b.mjs    # instant builds (or drop STUB for real Arc)
set -euo pipefail
cd "$(dirname "$0")"
export LANGFLOW_COMPONENTS_PATH="$PWD/components"
# 1.13 requires a superuser even for auto-login; local demo only.
export LANGFLOW_AUTO_LOGIN=true
export LANGFLOW_SUPERUSER="${LANGFLOW_SUPERUSER:-admin}"
export LANGFLOW_SUPERUSER_PASSWORD="${LANGFLOW_SUPERUSER_PASSWORD:-veil402demo}"
exec .venv/bin/langflow run --host "${LANGFLOW_HOST:-127.0.0.1}" --port "${LANGFLOW_PORT:-7860}"
