use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn read_json_line(reader: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response line");
    serde_json::from_str(line.trim()).expect("parse response json")
}

#[test]
fn stdio_initialize_then_initialized_then_list_tools() {
    let td = tempdir().expect("tempdir");

    let server_src = std::path::PathBuf::from(env!("CARGO_BIN_EXE_mcpcc-mcp-server"));
    let server_path = td.path().join("hello.mcp-server");
    std::fs::copy(&server_src, &server_path).expect("copy server binary");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&server_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&server_path, perms).expect("set permissions");
    }

    let tools = vec![serde_json::json!({
        "name": "hello.run_raw",
        "description": "Run hello",
        "inputSchema": {
            "type": "object",
            "properties": {
                "argv": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["argv"],
            "additionalProperties": false
        }
    })];

    let mcp_json = serde_json::json!({
        "mcpccVersion": "0.1.0",
        "mcpSpecVersion": "2025-11-25",
        "binary": { "path": "./hello" },
        "tools": tools,
    });

    let mcp_json_path = td.path().join("hello.mcp.json");
    std::fs::write(
        &mcp_json_path,
        serde_json::to_vec(&mcp_json).expect("serialize mcp json"),
    )
    .expect("write mcp.json");

    let mut child = Command::new(&server_path)
        .current_dir(td.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

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
            "params": {},
        })
    )
    .expect("write initialize");
    stdin.flush().expect("flush");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2025-11-25");

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/listTools",
            "params": {},
        })
    )
    .expect("write listTools");
    stdin.flush().expect("flush");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert_eq!(resp["error"]["code"], -32002);

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
    stdin.flush().expect("flush");

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/listTools",
            "params": {},
        })
    )
    .expect("write listTools after initialized");
    stdin.flush().expect("flush");

    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["tools"], mcp_json["tools"]);

    drop(stdin);
    let status = child.wait().expect("wait server");
    assert!(status.success());
}
