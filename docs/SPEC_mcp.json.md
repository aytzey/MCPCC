# `mcp.json` tool bundle specification (V1)

This document is the implementation contract for the `<bin>.mcp.json` files
written by `mcpcc` and consumed by `mcpcc-mcp-server`. It corresponds to PRD
§8–§9 and §13 (`tasks/prd-mcpcc.md`).

## File structure

```json
{
  "mcpccVersion": "0.1.0",
  "mcpSpecVersion": "2025-11-25",
  "binary": {
    "path": "./myprog",
    "defaultCwd": null
  },
  "tools": [ /* MCP Tool objects, see below */ ]
}
```

| Field | Type | Notes |
| --- | --- | --- |
| `mcpccVersion` | string | Version of the generating mcpcc |
| `mcpSpecVersion` | string | MCP spec version the bundle targets |
| `binary.path` | string | Executable to spawn. **Relative paths resolve against the directory containing the mcp.json**, not the server's cwd |
| `binary.defaultCwd` | string \| null | Working directory for spawned processes (relative ⇒ resolved against the bundle dir). `null` ⇒ inherit the server's cwd |
| `tools[]` | array | Non-empty. Each entry is an MCP Tool object plus the `x-mcpcc` extension |

## Tool objects

Standard MCP fields: `name`, `title`, `description`, `inputSchema`
(never null; JSON Schema object), `outputSchema`.

Tool names are normalized to `[a-zA-Z0-9._-]`, max 128 chars. Every bundle
contains at least the fallback tool `<base>.run_raw`; when extraction succeeds
a structured tool named `<base>` is emitted first.

### `outputSchema` / call results

Every call returns `structuredContent`:

```json
{
  "stdout": "string", "stderr": "string", "exitCode": 0, "durationMs": 0,
  "timedOut": false, "truncatedStdout": false, "truncatedStderr": false
}
```

`isError` is `true` when the spawn failed, the timeout was hit, or the exit
code is non-zero.

### `x-mcpcc` extension

```json
"x-mcpcc": {
  "kind": "structured" | "raw",
  "exec": { "timeoutMs": 30000, "maxStdoutBytes": 1048576, "maxStderrBytes": 1048576 },
  "argvMapping": {
    "options": [
      {
        "property": "color",
        "long": "--color",
        "short": "-c",
        "arg": "none" | "required" | "optional",
        "takesValue": true,
        "valueStyle": "separate" | "attached",
        "repeatable": false,
        "position": 2
      }
    ],
    "positionalProperty": "args"
  }
}
```

| Field | Meaning |
| --- | --- |
| `arg` | Option argument requirement. `none` ⇒ boolean flag; `required` ⇒ value always emitted; `optional` ⇒ empty string serializes the bare flag |
| `takesValue` | Legacy boolean form of `arg` (kept for compatibility; `arg` wins) |
| `valueStyle` | `separate` ⇒ `--flag value`; `attached` ⇒ `--flag=value` (short options attach without `=`: `-cvalue`). Generators emit `attached` for `arg: "optional"` because GNU getopt_long/argp only accept optional arguments attached |
| `repeatable` | Array inputs emit the flag once per element |
| `position` | Discovery order (informational; serialization follows array order) |
| `positionalProperty` | Schema property whose string-array is appended after all options (`argsParam` is the legacy alias) |

Argv serialization order is deterministic: options in `options[]` order, then
positionals in the given order. Option values may be JSON strings, numbers, or
booleans — non-strings are stringified.

### Raw tool input

```json
{ "argv": ["--flag", "value"], "stdin": "optional text piped to stdin" }
```

## Validation performed by the server

- `additionalProperties: false` ⇒ unknown argument properties are rejected
  (tool execution error, `isError: true`).
- `required` array ⇒ missing properties are rejected the same way.
- Unknown tool names ⇒ JSON-RPC **protocol error** `-32602`
  (per MCP Tools spec error handling).

## Server protocol surface

`mcpcc-mcp-server` speaks JSON-RPC 2.0 over stdio, newline-delimited.

| Method | Notes |
| --- | --- |
| `initialize` | Version negotiation: echoes the client's `protocolVersion` if known (2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25), else answers with the bundle's version |
| `notifications/initialized` | Spec name; legacy alias `initialized` accepted |
| `ping` | Valid in every lifecycle state, returns `{}` |
| `tools/list` | Spec name; legacy alias `tools/listTools` accepted |
| `tools/call` | Spec name; legacy alias `tools/callTool` accepted |

Requests before initialization complete get error `-32002` (except
`initialize` and `ping`).

## Annotation JSON schemas (PRD §11)

Embedded via `mcpcc_annot.h` into the `.mcpcc` ELF section, prefixed
`MCPCC_TOOL:` / `MCPCC_PARAM:`.

**Tool annotation** — `name` (required), `title`, `description`, `timeoutMs`,
`maxStdoutBytes`, `maxStderrBytes`.

**Param annotation** — `tool`, `property` (both required), `long`, `short`,
`takesValue`, `type` (`boolean|string|integer|number`), `repeatable`,
`required`, `description`.

Merge priority: annotation > argp/getopt extraction > fallback. LLM-generated
descriptions never overwrite annotation-provided descriptions, and never
overwrite extractor doc strings on structured tools.

## Extractor notes

The getopt extractor recognizes `getopt_long`, `getopt_long_only`, and PJLIB's
`pj_getopt_long` call shapes, with option tables declared as `struct option`
or `struct pj_getopt_option` (identical layouts). `has_arg` may be symbolic
(`no_argument`/…) or numeric (`0`/`1`/`2`). Duplicate long names keep the first
occurrence.

### Source discovery at link time

Extractors need source files, but link lines often contain only objects.
mcpcc recovers sources in this order per `.o` argument:

1. `<obj>.o.mcpcc-src` sidecar — written by mcpcc itself during the matching
   `-c` compile step (one absolute source path per line). Covers autoconf/make
   trees. Sidecars are advisory: writing them never fails the build, and stale
   ones are simply re-written on the next compile.
2. `something.c.o` → `something.c` sibling (object name embeds the extension).
3. CMake heuristic: `CMakeFiles/<tgt>.dir/<relpath>.c.o` → search `<relpath>.c`
   upwards from the build dir.

### LLM contract bounds

Per tool, at most 128 parameters are sent to (and expected back from) the LLM;
overflow is recorded in the manifest notes and the remaining schema properties
keep their extractor/placeholder descriptions.
