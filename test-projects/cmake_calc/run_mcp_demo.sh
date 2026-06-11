#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   ./run_mcp_demo.sh '10+7-3'
# or:
#   ./run_mcp_demo.sh            # defaults to 10+7-3

EXPR="${1:-10+7-3}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$ROOT_DIR/build"

if [ ! -d "$BUILD_DIR" ]; then
  echo "build dir not found: $BUILD_DIR" >&2
  echo "Run: mkdir -p build && cd build && cmake -DCMAKE_C_COMPILER=\"\$(pwd)/../mcpcc-gcc.sh\" .. && cmake --build ." >&2
  exit 2
fi

cd "$BUILD_DIR"

if [ ! -x "./calc.mcp-server" ]; then
  echo "./calc.mcp-server not found. Build first." >&2
  exit 2
fi

python3 - <<PY
import json, subprocess, sys
expr = ${EXPR@Q}

init={"jsonrpc":"2.0","id":1,"method":"initialize",
      "params":{"protocolVersion":"2025-11-25","capabilities":{},
                "clientInfo":{"name":"mcpcc-demo","version":"0"}}}
initialized={"jsonrpc":"2.0","method":"notifications/initialized"}
call={"jsonrpc":"2.0","id":2,"method":"tools/call",
      "params":{"name":"calc","arguments":{"expr":expr}}}

inp=(json.dumps(init)+"\n"+json.dumps(initialized)+"\n"+json.dumps(call)+"\n").encode()

p=subprocess.run(["./calc.mcp-server"], input=inp, capture_output=True, timeout=5)
lines=[ln for ln in p.stdout.decode(errors='ignore').splitlines() if ln.strip()]
if len(lines) < 2:
    sys.stderr.write("unexpected output from server\n")
    sys.stderr.write(p.stdout.decode(errors='ignore')+"\n")
    sys.stderr.write(p.stderr.decode(errors='ignore')+"\n")
    sys.exit(1)

resp=json.loads(lines[1])
sc=(resp.get('result') or {}).get('structuredContent') or {}

# Print just the program stdout (trim trailing newlines/spaces).
print((sc.get('stdout') or '').strip())

# Exit with the tool exitCode when possible.
try:
    sys.exit(int(sc.get('exitCode', 0)))
except Exception:
    sys.exit(0)
PY
