#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"  # MCPCC repo root
MCPCC_BIN="$ROOT/target/debug/mcpcc"

export MCPCC_ALLOW_NO_LLM=1

exec "$MCPCC_BIN" \
  --mcpcc-cc /usr/bin/gcc \
  --mcpcc-llm-mode off \
  --mcpcc-cache-dir /tmp/mcpcc-cmake-cache \
  --mcpcc-artifacts-dir . \
  -- "$@"
