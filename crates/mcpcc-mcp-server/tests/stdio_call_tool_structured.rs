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

fn start_server(
    dir: &std::path::Path,
    base: &str,
    mcp_json: serde_json::Value,
) -> std::process::Child {
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

    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..20 {
        match Command::new(&server_path)
            .current_dir(dir)
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

fn initialize_server(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
) {
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {},
        })
    )
    .expect("write initialize");
    stdin.flush().expect("flush initialize");

    let resp = read_json_line(reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        })
    )
    .expect("write initialized");
    stdin.flush().expect("flush initialized");
}

#[test]
#[cfg(unix)]
fn call_tool_structured_maps_argv_and_optional_args() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
for arg in "$@"; do
  printf 'ARG:%s\n' "$arg"
done
exit 0
"#,
    )
    .expect("write target binary");
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
                    "mode": { "type": "string" },
                    "verbose": { "type": "boolean" },
                    "color": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["mode"],
                "additionalProperties": false
            },
            "outputSchema": { "type": "object" },
            "x-mcpcc": {
                "exec": { "timeoutMs": 5000, "maxStdoutBytes": 1024, "maxStderrBytes": 1024 },
                "argvMapping": {
                    "options": [
                        { "property": "mode", "long": "--mode", "takesValue": true, "valueStyle": "separate", "repeatable": false, "position": 0 },
                        { "property": "verbose", "long": "--verbose", "takesValue": false, "valueStyle": "separate", "repeatable": false, "position": 1 },
                        { "property": "color", "long": "--color", "takesValue": true, "valueStyle": "separate", "repeatable": false, "position": 2 }
                    ],
                    "positionalProperty": "args"
                }
            }
        }]
    });

    let mut child = start_server(td.path(), "hello", mcp_json);
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    initialize_server(&mut stdin, &mut reader);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/callTool",
            "params": {
                "name": "hello",
                "arguments": { "mode": "fast", "verbose": true, "args": ["one"] }
            },
        })
    )
    .expect("write callTool omit optional");
    stdin.flush().expect("flush callTool omit optional");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:--mode\nARG:fast\nARG:--verbose\nARG:one\n"
    );

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/callTool",
            "params": {
                "name": "hello",
                "arguments": { "mode": "fast", "color": "", "args": [] }
            },
        })
    )
    .expect("write callTool empty optional");
    stdin.flush().expect("flush callTool empty optional");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:--mode\nARG:fast\nARG:--color\n"
    );

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/callTool",
            "params": {
                "name": "hello",
                "arguments": { "mode": "fast", "color": "red", "args": ["two", "three"] }
            },
        })
    )
    .expect("write callTool value optional");
    stdin.flush().expect("flush callTool value optional");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 4);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:--mode\nARG:fast\nARG:--color\nARG:red\nARG:two\nARG:three\n"
    );

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn call_tool_structured_rejects_unknown_properties() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
echo "should not run"
exit 0
"#,
    )
    .expect("write target binary");
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
                    "mode": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["mode"],
                "additionalProperties": false
            },
            "outputSchema": { "type": "object" },
            "x-mcpcc": {
                "argvMapping": {
                    "options": [
                        { "property": "mode", "long": "--mode", "takesValue": true, "valueStyle": "separate", "repeatable": false, "position": 0 }
                    ],
                    "positionalProperty": "args"
                }
            }
        }]
    });

    let mut child = start_server(td.path(), "hello", mcp_json);
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    initialize_server(&mut stdin, &mut reader);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/callTool",
            "params": {
                "name": "hello",
                "arguments": { "mode": "fast", "bogus": "nope" }
            },
        })
    )
    .expect("write callTool");
    stdin.flush().expect("flush callTool");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], true);
    assert_eq!(resp["result"]["structuredContent"]["exitCode"], -1);

    let stderr = resp["result"]["structuredContent"]["stderr"]
        .as_str()
        .expect("stderr string");
    assert!(stderr.contains("unknown argument property"));
    assert!(stderr.contains("bogus"));

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn call_tool_structured_accepts_legacy_args_param_and_param_key() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
for arg in "$@"; do
  printf 'ARG:%s\n' "$arg"
done
exit 0
"#,
    )
    .expect("write target binary");
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
                    "mode": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["mode"],
                "additionalProperties": false
            },
            "outputSchema": { "type": "object" },
            "x-mcpcc": {
                "argvMapping": {
                    "options": [
                        { "param": "mode", "long": "--mode", "arg": "required" }
                    ],
                    "argsParam": "args"
                }
            }
        }]
    });

    let mut child = start_server(td.path(), "hello", mcp_json);
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    initialize_server(&mut stdin, &mut reader);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/callTool",
            "params": {
                "name": "hello",
                "arguments": { "mode": "fast", "args": ["one"] }
            },
        })
    )
    .expect("write callTool");
    stdin.flush().expect("flush callTool");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"],
        "ARG:--mode\nARG:fast\nARG:one\n"
    );

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}
