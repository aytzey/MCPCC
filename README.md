# mcpcc

**Compile any C/C++ program — get an AI-usable MCP tool for free.**

`mcpcc` is a drop-in gcc/clang wrapper. It compiles your code exactly like the
underlying compiler would (same arguments, same binary, same exit code), and
whenever the link step produces an executable it additionally emits:

| Artifact | Purpose |
| --- | --- |
| `<bin>.mcp.json` | Tool bundle: MCP tool definitions + argv mapping for the binary |
| `<bin>.mcp-server` | Self-contained MCP server (stdio) that exposes those tools and spawns the binary |
| `<bin>.mcpcc-manifest.json` | Build/analysis/LLM provenance for debugging |

Any MCP client — Claude Code, Claude Desktop, or anything else speaking
[MCP](https://modelcontextprotocol.io) over stdio — can then call your freshly
compiled binary as a typed tool, without you writing a single line of glue.

```
┌──────────┐   mcpcc -- main.c -o calc    ┌──────────────────────┐
│  C code  │ ───────────────────────────▶ │ calc                 │  normal binary
└──────────┘                              │ calc.mcp.json        │  tool schema
                                          │ calc.mcp-server      │  MCP stdio server
                                          │ calc.mcpcc-manifest… │  provenance
                                          └──────────────────────┘
                                                      ▲
                                   MCP client (Claude │ tools/list, tools/call
                                   Code/Desktop, …) ──┘
```

## Quickstart

```bash
cargo build --workspace            # builds `mcpcc` and `mcpcc-mcp-server`
export PATH="$PWD/target/debug:$PATH"

# LLM descriptions are required by default (see "LLM descriptions" below).
# For a first offline run:
export MCPCC_ALLOW_NO_LLM=1

mkdir -p /tmp/demo
mcpcc --mcpcc-llm-mode off -- samples/getopt_long.c -o /tmp/demo/cli
ls /tmp/demo
# cli  cli.mcp.json  cli.mcp-server  cli.mcpcc-manifest.json
```

Talk to the generated server with any MCP client. Manually:

```bash
cd /tmp/demo
printf '%s\n%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cli","arguments":{"verbose":true,"output":"out.txt","args":["a","b"]}}}' \
  | ./cli.mcp-server
```

### Use with Claude Code

```bash
claude mcp add my-calc -- /abs/path/to/calc.mcp-server
```

### Use with Claude Desktop

```json
{
  "mcpServers": {
    "my-calc": { "command": "/abs/path/to/calc.mcp-server" }
  }
}
```

The server locates `<bin>.mcp.json` next to its own executable and resolves a
relative `binary.path` against that directory, so it works no matter which
working directory the MCP client launches it from.

## What the AI sees

For a CLI that uses `getopt_long` or `argp`, mcpcc extracts the option table and
publishes a **structured tool** whose JSON Schema mirrors the real flags:

```json
{
  "name": "cli",
  "description": "…LLM-generated…",
  "inputSchema": {
    "type": "object",
    "properties": {
      "verbose": { "type": "boolean" },
      "output":  { "type": "string" },
      "color":   { "type": "string" },
      "args":    { "type": "array", "items": { "type": "string" } }
    },
    "additionalProperties": false
  }
}
```

Calls map deterministically to argv (`--output value`, optional-argument
options serialize attached as `--color=value`, then positionals from `args`).

Every binary additionally gets a **`<name>.run_raw`** fallback tool taking a raw
`argv: string[]` plus optional `stdin: string`, so even programs with no
recognizable CLI parser (or interactive stdin-driven ones) are immediately
usable.

The result of every call is returned as MCP `structuredContent`:

```json
{ "stdout": "…", "stderr": "…", "exitCode": 0, "durationMs": 12,
  "timedOut": false, "truncatedStdout": false, "truncatedStderr": false }
```

## CLI reference

```
mcpcc [MCPCC_FLAGS...] -- [COMPILER_ARGS...]
mcpcc [MIXED_ARGS...]        # flags prefixed --mcpcc- are consumed, rest passes through
```

| Flag | Meaning |
| --- | --- |
| `--mcpcc-cc <path>` | Underlying compiler (default: `$MCPCC_CC`, `$CC`, `clang`, `gcc`) |
| `--mcpcc-print-cc` | Print resolved compiler path and exit |
| `--mcpcc-artifacts-dir <dir>` | Where to write artifacts (default: binary's directory) |
| `--mcpcc-mcp-json-out <path>` | Override mcp.json path |
| `--mcpcc-server-out <path>` | Override server binary path |
| `--mcpcc-manifest-out <path>` | Override manifest path |
| `--mcpcc-llm-mode <mode>` | `required` (default) \| `best-effort` \| `off` |
| `--mcpcc-llm-model <id>` | OpenRouter model id (default: `openai/gpt-4o-mini`) |
| `--mcpcc-cache-dir <dir>` | LLM cache (default: `~/.cache/mcpcc`) |
| `--mcpcc-verbose` | Detailed diagnostics on stderr |
| `--mcpcc-version` / `--mcpcc-help` | Version / usage |

Environment variables: `OPENROUTER_API_KEY`, `MCPCC_CC`, `MCPCC_LLM_MODE`,
`MCPCC_LLM_MODEL`, `MCPCC_CACHE_DIR`, `MCPCC_ARTIFACTS_DIR`,
`MCPCC_ALLOW_NO_LLM` (required for `off`), `MCPCC_OPENROUTER_BASE_URL`.
Flags always win over environment variables.

Exit codes: compiler failures propagate unchanged; wrapper usage errors exit 2;
"compiled fine but MCP artifact generation failed" exits 70.

Artifacts are only produced for executable links — `-c`, `-E`, `-S`, `-shared`,
`-r`, `-fsyntax-only`, `-M`, `-MM` invocations pass straight through.
`@response-file` link lines (CMake + Ninja) are expanded for analysis.

## LLM descriptions

Tool and parameter descriptions are generated with an LLM via
[OpenRouter](https://openrouter.ai) (`OPENROUTER_API_KEY`). Only a compact
analysis summary is sent — never your full source code. Results are cached
under the cache dir keyed by `sha256(promptVersion + model + summary)`, so
rebuilding identical code never re-calls the API.

- `required` (default): no key / failed call ⇒ build exits 70.
- `best-effort`: falls back to deterministic placeholder descriptions
  (recorded in the manifest).
- `off`: placeholders only; needs `MCPCC_ALLOW_NO_LLM=1` (CI/test escape hatch).

## Overriding extraction with annotations

Include [`mcpcc_annot.h`](mcpcc_annot.h) and add JSON annotations that are
embedded into a `.mcpcc` ELF section (they don't change program behavior):

```c
#include "mcpcc_annot.h"

MCPCC_TOOL_JSON("{\"name\":\"myprog\",\"description\":\"Does the thing\",\"timeoutMs\":5000}");
MCPCC_PARAM_JSON("{\"tool\":\"myprog\",\"property\":\"level\",\"long\":\"--level\",\"type\":\"integer\",\"required\":true,\"description\":\"Detail level\"}");
```

Merge priority is deterministic: **annotation > argp/getopt extraction >
fallback**. See [`docs/SPEC_mcp.json.md`](docs/SPEC_mcp.json.md) for the full
bundle format, annotation schema, and server protocol surface.

## Using mcpcc with CMake

Point CMake at the wrapper script (see
[`test-projects/cmake_calc`](test-projects/cmake_calc) for a working example):

```bash
cd test-projects/cmake_calc
mkdir -p build && cd build
cmake -DCMAKE_C_COMPILER="$(pwd)/../mcpcc-gcc.sh" ..   # CMake needs a full path
cmake --build .
cd .. && ./run_mcp_demo.sh '2*(3+4)'   # calls the calc tool through the MCP server → 14
```

For multi-file CMake projects, mcpcc recovers original source paths from the
`CMakeFiles/<target>.dir/….c.o` object names at link time and runs the
extractors on them.

## Repository layout

```
crates/mcpcc/             compiler wrapper + extractors + bundle/manifest generation
crates/mcpcc-mcp-server/  generic stdio MCP server (copied per binary)
mcpcc_annot.h             annotation header for overrides
samples/                  golden C samples (argp, getopt_long, annotations, none)
test-projects/cmake_calc/ end-to-end CMake demo project
scripts/e2e.sh            offline end-to-end check over all samples
tasks/prd-mcpcc.md        product requirements (V1)
docs/SPEC_mcp.json.md     mcp.json bundle + server protocol specification
```

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
./scripts/e2e.sh           # offline (llm-mode off)
```

## Security note

The generated server executes the target binary on the host with the caller's
privileges — running a tool means running that program. There is no sandbox in
V1 (`x-mcpcc.exec` enforces timeout and output-size limits only). Only expose
binaries you trust to MCP clients, and treat the artifacts like the executables
they wrap.
