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
fn call_tool_run_raw_captures_stdout_stderr_exit_code() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
for arg in "$@"; do
  printf 'OUT:%s\n' "$arg"
done
for arg in "$@"; do
  printf 'ERR:%s\n' "$arg" >&2
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
            "name": "hello.run_raw",
            "title": "hello.run_raw",
            "description": "Run hello",
            "inputSchema": {
                "type": "object",
                "properties": { "argv": { "type": "array", "items": { "type": "string" } } },
                "required": ["argv"],
                "additionalProperties": false
            },
            "outputSchema": { "type": "object" },
            "x-mcpcc": {
                "kind": "raw",
                "exec": { "timeoutMs": 5000, "maxStdoutBytes": 1024, "maxStderrBytes": 1024 }
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
                "name": "hello.run_raw",
                "arguments": { "argv": ["one", "two"] }
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
        "OUT:one\nOUT:two\n"
    );
    assert_eq!(
        resp["result"]["structuredContent"]["stderr"],
        "ERR:one\nERR:two\n"
    );
    assert_eq!(resp["result"]["structuredContent"]["exitCode"], 0);
    assert_eq!(resp["result"]["structuredContent"]["timedOut"], false);
    assert_eq!(
        resp["result"]["structuredContent"]["truncatedStdout"],
        false
    );
    assert_eq!(
        resp["result"]["structuredContent"]["truncatedStderr"],
        false
    );

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn call_tool_run_raw_enforces_timeout() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
sleep 1
echo "should not complete"
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
            "name": "hello.run_raw",
            "title": "hello.run_raw",
            "description": "Run hello",
            "inputSchema": {
                "type": "object",
                "properties": { "argv": { "type": "array", "items": { "type": "string" } } },
                "required": ["argv"],
                "additionalProperties": false
            },
            "outputSchema": { "type": "object" },
            "x-mcpcc": {
                "kind": "raw",
                "exec": { "timeoutMs": 10, "maxStdoutBytes": 1024, "maxStderrBytes": 1024 }
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
                "name": "hello.run_raw",
                "arguments": { "argv": [] }
            },
        })
    )
    .expect("write callTool");
    stdin.flush().expect("flush callTool");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], true);
    assert_eq!(resp["result"]["structuredContent"]["timedOut"], true);

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}

#[test]
#[cfg(unix)]
fn call_tool_run_raw_enforces_stdout_stderr_limits() {
    let td = tempdir().expect("tempdir");

    let bin_path = td.path().join("hello");
    std::fs::write(
        &bin_path,
        br#"#!/bin/sh
set -eu
i=0
while [ "$i" -lt 50 ]; do
  printf 'a'
  i=$((i + 1))
done
i=0
while [ "$i" -lt 30 ]; do
  printf 'b' >&2
  i=$((i + 1))
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
            "name": "hello.run_raw",
            "title": "hello.run_raw",
            "description": "Run hello",
            "inputSchema": {
                "type": "object",
                "properties": { "argv": { "type": "array", "items": { "type": "string" } } },
                "required": ["argv"],
                "additionalProperties": false
            },
            "outputSchema": { "type": "object" },
            "x-mcpcc": {
                "kind": "raw",
                "exec": { "timeoutMs": 5000, "maxStdoutBytes": 10, "maxStderrBytes": 5 }
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
                "name": "hello.run_raw",
                "arguments": { "argv": [] }
            },
        })
    )
    .expect("write callTool");
    stdin.flush().expect("flush callTool");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["result"]["isError"], false);
    assert_eq!(resp["result"]["structuredContent"]["truncatedStdout"], true);
    assert_eq!(resp["result"]["structuredContent"]["truncatedStderr"], true);
    assert_eq!(
        resp["result"]["structuredContent"]["stdout"]
            .as_str()
            .expect("stdout string")
            .len(),
        10
    );
    assert_eq!(
        resp["result"]["structuredContent"]["stderr"]
            .as_str()
            .expect("stderr string")
            .len(),
        5
    );

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}
