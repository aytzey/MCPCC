#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

ARTIFACTS_DIR="${ROOT_DIR}/.e2e-out"
SAMPLES_DIR="${ROOT_DIR}/samples"
BIN_DIR="${ARTIFACTS_DIR}/bin"
CACHE_DIR="${ARTIFACTS_DIR}/cache"

rm -rf "$ARTIFACTS_DIR"
mkdir -p "$BIN_DIR" "$CACHE_DIR"

echo "[e2e] building workspace..."
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
cargo build --workspace

MCPCC_BIN="$ROOT_DIR/target/debug/mcpcc"
if [ ! -x "$MCPCC_BIN" ]; then
  echo "[e2e] ERROR: mcpcc binary not found at $MCPCC_BIN" >&2
  exit 1
fi

# Pick a compiler for real builds. If none found, fall back to a stub fakecc.
CC_BIN=""
if command -v clang >/dev/null 2>&1; then
  CC_BIN="$(command -v clang)"
elif command -v gcc >/dev/null 2>&1; then
  CC_BIN="$(command -v gcc)"
fi

FAKECC="$BIN_DIR/fakecc"
if [ -z "$CC_BIN" ]; then
  echo "[e2e] no clang/gcc found; using fakecc"
  cat >"$FAKECC" <<'SH'
#!/bin/sh
set -eu
out="a.out"
prev=""
for a in "$@"; do
  if [ "$prev" = "-o" ]; then
    out="$a"
    prev=""
    continue
  fi
  case "$a" in
    -o) prev="-o";;
    -o*) out="${a#-o}";;
  esac
done
mkdir -p "$(dirname "$out")"
cat >"$out" <<'EOS'
#!/bin/sh
for a in "$@"; do
  echo "ARG:$a"
done
exit 0
EOS
chmod +x "$out"
exit 0
SH
  chmod +x "$FAKECC"
  CC_BIN="$FAKECC"
fi

echo "[e2e] using compiler: $CC_BIN"

# LLM mode: default to off for deterministic runs. Requires MCPCC_ALLOW_NO_LLM=1.
LLM_MODE="${MCPCC_E2E_LLM_MODE:-off}"
if [ "$LLM_MODE" = "off" ]; then
  export MCPCC_ALLOW_NO_LLM=1
fi

echo "[e2e] compiling samples + validating servers..."

for src in "$SAMPLES_DIR"/*.c; do
  name="$(basename "$src" .c)"
  out="$BIN_DIR/$name"

  echo "[e2e] mcpcc building $name"
  "$MCPCC_BIN" \
    --mcpcc-cc "$CC_BIN" \
    --mcpcc-llm-mode "$LLM_MODE" \
    --mcpcc-cache-dir "$CACHE_DIR" \
    --mcpcc-artifacts-dir "$BIN_DIR" \
    -- "$src" -o "$out"

  test -f "$BIN_DIR/$name.mcp.json"
  test -f "$BIN_DIR/$name.mcp-server"

  # Validate server: run initialize + callTool (raw) in one shot, parse 2 JSON lines.
  NAME="$name" BIN_DIR_PY="$BIN_DIR" python3 - <<'PY'
import json, subprocess, os

name = os.environ["NAME"]
bin_dir = os.environ["BIN_DIR_PY"]
server = os.path.join(bin_dir, f"{name}.mcp-server")
raw_tool = f"{name}.run_raw"

init = {"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},
                  "clientInfo":{"name":"mcpcc-e2e","version":"0"}}}
initialized = {"jsonrpc":"2.0","method":"notifications/initialized"}
call = {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":raw_tool,"arguments":{"argv":["--help"]}}}

inp = (json.dumps(init)+"\n"+json.dumps(initialized)+"\n"+json.dumps(call)+"\n").encode()

try:
    p = subprocess.run([server], input=inp, capture_output=True, timeout=5)
except subprocess.TimeoutExpired:
    raise SystemExit(f"{name}: server timed out")

out_lines = [ln for ln in p.stdout.decode(errors="ignore").splitlines() if ln.strip()]
if len(out_lines) < 2:
    raise SystemExit(f"{name}: expected >=2 JSON lines, got {len(out_lines)}; stderr={p.stderr.decode(errors='ignore')}")

# First response should be initialize result, second should be callTool result.
try:
    r1 = json.loads(out_lines[0])
    r2 = json.loads(out_lines[1])
except Exception as e:
    raise SystemExit(f"{name}: failed to parse JSON lines: {e}; stdout={out_lines}")

if r1.get("id") != 1:
    raise SystemExit(f"{name}: unexpected first response id={r1.get('id')}")
if r2.get("id") != 2:
    raise SystemExit(f"{name}: unexpected second response id={r2.get('id')}")

sc = (r2.get("result") or {}).get("structuredContent") or {}
if "exitCode" not in sc:
    raise SystemExit(f"{name}: missing exitCode in structuredContent")

print("ok")
PY

done

echo "[e2e] OK"
