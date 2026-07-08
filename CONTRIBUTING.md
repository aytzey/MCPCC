# Contributing to mcpcc

Thanks for your interest! Bug reports, extractor improvements (new CLI-parser
dialects), and real-world build-system reports are all welcome.

## Getting set up

```bash
git clone https://github.com/aytzey/MCPCC && cd MCPCC
cargo build --workspace
```

You need a C compiler (`clang` or `gcc`) on `PATH` for the integration tests
and the end-to-end script. No API key is required for development — everything
runs offline with `--mcpcc-llm-mode off`.

## Before you open a PR

Run the same checks CI runs:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/e2e.sh
```

All four must pass. CI additionally builds the
[`test-projects/cmake_calc`](test-projects/cmake_calc) demo through CMake and
verifies a real MCP `tools/call` round-trip.

## What a good change looks like

- **Behavior changes come with tests.** Extractor changes get a golden sample
  in [`samples/`](samples) plus an integration test under
  [`crates/mcpcc/tests/`](crates/mcpcc/tests); server protocol changes get a
  stdio test under [`crates/mcpcc-mcp-server/tests/`](crates/mcpcc-mcp-server/tests).
- **The bundle format is a contract.** Anything that changes `mcp.json`, the
  annotation schema, or the server protocol surface must update
  [`docs/SPEC_mcp.json.md`](docs/SPEC_mcp.json.md) in the same PR.
- **Compiler passthrough is sacred.** `mcpcc` must compile exactly like the
  underlying compiler: same arguments, same binary, same exit code. Changes
  that could affect passthrough need a test in
  [`crates/mcpcc/tests/passthrough.rs`](crates/mcpcc/tests/passthrough.rs).
- Keep commits focused and messages descriptive — the existing history is the
  style guide.

## Reporting issues

For extraction bugs, the fastest path to a fix is a minimal C sample plus the
generated `*.mcp.json` and `*.mcpcc-manifest.json` (the manifest records which
extractor ran and why). For build-system integration issues, include the exact
`configure`/`cmake` invocation and the wrapper script you used.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
