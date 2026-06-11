//! Coverage for the standard MCP protocol surface (spec method names,
//! version negotiation, ping, protocol errors) and for spawn behavior that
//! real MCP clients depend on (bundle-relative binary paths, stdin piping,
//! attached option values, scalar coercion).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::tempdir;

fn read_json_line(reader: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response line");
    serde_json::from_str(line.trim()).expect("parse response json")
}

fn chmod_exe(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("set permissions");
    }
}

fn write_bundle(
    dir: &std::path::Path,
    base: &str,
    mcp_json: serde_json::Value,
) -> std::path::PathBuf {
    let server_src = std::path::PathBuf::from(env!("CARGO_BIN_EXE_mcpcc-mcp-server"));
    let server_path = dir.join(format!("{base}.mcp-server"));
    std::fs::copy(&server_src, &server_path).expect("copy server binary");
    chmod_exe(&server_path);

    let mcp_json_path = dir.join(format!("{base}.mcp.json"));
    std::fs::write(
        &mcp_json_path,
        serde_json::to_vec(&mcp_json).expect("serialize mcp json"),
    )
    .expect("write mcp.json");

    server_path
}

fn spawn_server(server_path: &std::path::Path, cwd: &std::path::Path) -> std::process::Child {
    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..20 {
        match Command::new(server_path)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => return child,
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("spawn server: {err}"),
        }
    }
    panic!(
        "spawn server: {}",
        last_err
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "ExecutableFileBusy".to_string())
    );
}

fn echo_args_binary() -> &'static [u8] {
    br#"#!/bin/sh
set -eu
for arg in "$@"; do
  printf 'ARG:%s\n' "$arg"
done
exit 0
"#
}

fn run_raw_bundle_json(base: &str) -> serde_json::Value {
    serde_json::json!({
        "mcpccVersion": "0.1.0",
        "mcpSpecVersion": "2025-11-25",
        "binary": { "path": format!("./{base}"), "defaultCwd": null },
        "tools": [{
            "name": format!("{base}.run_raw"),
            "title": format!("{base}.run_raw"),
            "description": "Run with raw argv",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "argv": { "type": "array", "items": { "type": "string" } },
                    "stdin": { "type": "string" }
                },
                "required": ["argv"],
                "additionalProperties": false
            },
            "x-mcpcc": { "kind": "raw" }
        }]
    })
}

#[test]
#[cfg(unix)]
fn standard_lifecycle_ping_list_and_call() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(&bin_path, echo_args_binary()).expect("write target binary");
    chmod_exe(&bin_path);

    let server_path = write_bundle(td.path(), "hello", run_raw_bundle_json("hello"));
    let mut child = spawn_server(&server_path, td.path());
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    // Ping must work before initialize.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "id": 0, "method": "ping" })
    )
    .expect("write ping");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 0);
    assert_eq!(resp["result"], serde_json::json!({}));

    // Version negotiation echoes a known client version.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "1.0.0" }
            },
        })
    )
    .expect("write initialize");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        resp["result"]["capabilities"]["tools"]["listChanged"],
        false
    );

    // Spec notification name.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
    )
    .expect("write notifications/initialized");
    stdin.flush().expect("flush");

    // Spec list method.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })
    )
    .expect("write tools/list");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["tools"][0]["name"], "hello.run_raw");

    // Spec call method.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "hello.run_raw", "arguments": { "argv": ["one", "two"] } },
        })
    )
    .expect("write tools/call");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:one\nARG:two\n"
    );

    // Unknown tool must be a protocol error per the MCP Tools spec.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "nope", "arguments": { "argv": [] } },
        })
    )
    .expect("write tools/call unknown");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 4);
    assert_eq!(resp["error"]["code"], -32602);
    assert!(
        resp["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("Unknown tool: nope"),
        "unexpected error message: {}",
        resp["error"]["message"]
    );

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn initialize_falls_back_to_bundle_version_for_unknown_client_version() {
    let td = tempdir().expect("tempdir");
    let bin_path = td.path().join("hello");
    std::fs::write(&bin_path, echo_args_binary()).expect("write target binary");
    chmod_exe(&bin_path);

    let server_path = write_bundle(td.path(), "hello", run_raw_bundle_json("hello"));
    let mut child = spawn_server(&server_path, td.path());
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "1.0.0" },
        })
    )
    .expect("write initialize");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["result"]["protocolVersion"], "2025-11-25");

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn relative_binary_path_resolves_against_bundle_dir_not_server_cwd() {
    let td = tempdir().expect("tempdir");
    let bundle_dir = td.path().join("bundle");
    let other_cwd = td.path().join("elsewhere");
    std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    std::fs::create_dir_all(&other_cwd).expect("create other cwd");

    let bin_path = bundle_dir.join("hello");
    std::fs::write(&bin_path, echo_args_binary()).expect("write target binary");
    chmod_exe(&bin_path);

    let server_path = write_bundle(&bundle_dir, "hello", run_raw_bundle_json("hello"));
    // Launch the server from an unrelated cwd, as MCP clients do.
    let mut child = spawn_server(&server_path, &other_cwd);
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })
    )
    .expect("write initialize");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
    )
    .expect("write initialized");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "hello.run_raw", "arguments": { "argv": ["works"] } },
        })
    )
    .expect("write tools/call");
    stdin.flush().expect("flush");

    let _init = read_json_line(&mut reader);
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(resp["result"]["structuredContent"]["stdout"], "ARG:works\n");

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn run_raw_pipes_stdin_to_binary() {
    let td = tempdir().expect("tempdir");
    let bin_path = td.path().join("cat");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
while IFS= read -r line; do
  printf 'IN:%s\n' "$line"
done
exit 0
"#,
    )
    .expect("write target binary");
    chmod_exe(&bin_path);

    let server_path = write_bundle(td.path(), "cat", run_raw_bundle_json("cat"));
    let mut child = spawn_server(&server_path, td.path());
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })
    )
    .expect("write initialize");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
    )
    .expect("write initialized");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "cat.run_raw",
                "arguments": { "argv": [], "stdin": "10+7-3\n" }
            },
        })
    )
    .expect("write tools/call");
    stdin.flush().expect("flush");

    let _init = read_json_line(&mut reader);
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(resp["result"]["structuredContent"]["stdout"], "IN:10+7-3\n");

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn structured_call_attached_optional_and_scalar_coercion() {
    let td = tempdir().expect("tempdir");
    let bin_path = td.path().join("hello");
    std::fs::write(&bin_path, echo_args_binary()).expect("write target binary");
    chmod_exe(&bin_path);

    let mcp_json = serde_json::json!({
        "mcpccVersion": "0.1.0",
        "mcpSpecVersion": "2025-11-25",
        "binary": { "path": "./hello", "defaultCwd": null },
        "tools": [{
            "name": "hello",
            "title": "hello",
            "description": "Run hello structured",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "color": { "type": "string" },
                    "level": { "type": "integer" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "additionalProperties": false
            },
            "x-mcpcc": {
                "argvMapping": {
                    "options": [
                        { "property": "color", "long": "--color", "arg": "optional", "valueStyle": "attached", "position": 0 },
                        { "property": "level", "long": "--level", "arg": "required", "valueStyle": "separate", "position": 1 }
                    ],
                    "positionalProperty": "args"
                }
            }
        }]
    });

    let server_path = write_bundle(td.path(), "hello", mcp_json);
    let mut child = spawn_server(&server_path, td.path());
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })
    )
    .expect("write initialize");
    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
    )
    .expect("write initialized");

    // Optional arg with a value serializes attached; integer values are
    // stringified instead of rejected.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "hello",
                "arguments": { "color": "red", "level": 3, "args": ["x"] }
            },
        })
    )
    .expect("write tools/call");
    stdin.flush().expect("flush");

    let _init = read_json_line(&mut reader);
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:--color=red\nARG:--level\nARG:3\nARG:x\n"
    );

    // Empty optional value still serializes as the bare flag.
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "hello", "arguments": { "color": "" } },
        })
    )
    .expect("write tools/call");
    stdin.flush().expect("flush");
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["id"], 3);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:--color\n"
    );

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}
