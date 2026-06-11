use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}-{pid}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/mcpcc is two levels under repo root")
        .to_path_buf()
}

fn chmod_exe(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("set permissions");
    }
}

fn read_json_line(reader: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response line");
    serde_json::from_str(line.trim()).expect("parse response json")
}

fn write_json(stdin: &mut std::process::ChildStdin, value: serde_json::Value) {
    writeln!(stdin, "{}", value).expect("write json");
    stdin.flush().expect("flush");
}

fn initialize_server(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
) {
    write_json(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {},
        }),
    );
    let resp = read_json_line(reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);

    write_json(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        }),
    );
}

fn assert_prd_tool_shape(tool: &serde_json::Value) {
    assert!(
        tool.get("title").and_then(|v| v.as_str()).is_some(),
        "tool must have title"
    );
    assert!(
        tool.get("outputSchema")
            .and_then(|v| v.as_object())
            .is_some(),
        "tool must have outputSchema"
    );

    let input_schema = tool
        .get("inputSchema")
        .and_then(|v| v.as_object())
        .expect("tool must have inputSchema object");
    assert_eq!(
        input_schema
            .get("additionalProperties")
            .and_then(|v| v.as_bool()),
        Some(false),
        "inputSchema.additionalProperties must be false"
    );

    assert!(
        tool.get("x-mcpcc")
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str())
            .is_some(),
        "tool must have x-mcpcc.kind"
    );
}

fn compile_sample(td: &TempDir, name: &str, sample: &Path) -> (PathBuf, PathBuf) {
    let bin = env!("CARGO_BIN_EXE_mcpcc");

    let out_bin = td.path.join(name);
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env("MCPCC_ALLOW_NO_LLM", "1")
        .arg("--mcpcc-cc")
        .arg("cc")
        .arg("--mcpcc-llm-mode")
        .arg("off")
        .arg("--")
        .arg(sample)
        .arg("-O0")
        .arg("-g")
        .arg("-o")
        .arg(&out_bin)
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "mcpcc compile failed for {name}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let server_path = td.path.join(format!("{name}.mcp-server"));
    assert!(
        server_path.exists(),
        "missing server binary: {}",
        server_path.display()
    );
    chmod_exe(&server_path);

    let mcp_json_path = td.path.join(format!("{name}.mcp.json"));
    assert!(
        mcp_json_path.exists(),
        "missing mcp json: {}",
        mcp_json_path.display()
    );

    (server_path, mcp_json_path)
}

fn list_tools(
    server_path: &Path,
    cwd: &Path,
) -> (
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
    Vec<serde_json::Value>,
) {
    let mut child = Command::new(server_path)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);

    initialize_server(&mut stdin, &mut reader);

    write_json(
        &mut stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/listTools",
            "params": {},
        }),
    );
    let resp = read_json_line(&mut reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("result.tools array")
        .to_vec();

    (child, stdin, reader, tools)
}

fn call_tool(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    id: i64,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    write_json(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/callTool",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        }),
    );
    let resp = read_json_line(reader);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], id);
    resp
}

#[test]
#[cfg(unix)]
fn e2e_mcpcc_generates_bundle_and_stdio_server_can_list_and_call_tools() {
    let root = repo_root();

    // Smoke check: "cc" must exist for the end-to-end build.
    let cc_ok = Command::new("cc").arg("--version").output().is_ok();
    if !cc_ok {
        eprintln!("skipping: cc not available in PATH");
        return;
    }

    let samples = [
        (
            "argp",
            root.join("samples/argp.c"),
            Some(serde_json::json!({
                "output": "file.txt",
                "color": "",
                "verbose": true,
                "args": ["one"],
            })),
            Some("ARG:--output\nARG:file.txt\nARG:--color\nARG:--verbose\nARG:one\n"),
        ),
        (
            "getopt_long",
            root.join("samples/getopt_long.c"),
            Some(serde_json::json!({
                "verbose": true,
                "output": "file.txt",
                "color": "",
                "args": ["one"],
            })),
            Some("ARG:--verbose\nARG:--output\nARG:file.txt\nARG:--color\nARG:one\n"),
        ),
        (
            "annotated",
            root.join("samples/annotated.c"),
            Some(serde_json::json!({
                "verbose": true,
                "output": "file.txt",
                "args": ["one"],
            })),
            Some("ARG:--verbose\nARG:--output\nARG:file.txt\nARG:one\n"),
        ),
        (
            "none",
            root.join("samples/none.c"),
            None,
            Some("ARG:one\nARG:two\n"),
        ),
    ];

    for (name, sample_path, structured_args, expected_stdout) in samples {
        assert!(
            sample_path.exists(),
            "missing sample: {}",
            sample_path.display()
        );

        let td = TempDir::new(&format!("mcpcc-e2e-{name}"));
        let (server_path, mcp_json_path) = compile_sample(&td, name, &sample_path);

        let mcp_json_bytes = std::fs::read(&mcp_json_path).expect("read mcp json");
        let mcp_json: serde_json::Value =
            serde_json::from_slice(&mcp_json_bytes).expect("parse mcp json");
        assert_eq!(mcp_json["binary"]["defaultCwd"], serde_json::Value::Null);

        let (mut child, mut stdin, mut reader, tools) = list_tools(&server_path, &td.path);
        assert!(!tools.is_empty(), "tools must be non-empty");
        for tool in &tools {
            assert_prd_tool_shape(tool);
        }

        let structured_name = name.to_string();
        let run_raw_name = format!("{name}.run_raw");

        let call_resp = if let Some(args) = structured_args {
            call_tool(&mut stdin, &mut reader, 3, &structured_name, args)
        } else {
            call_tool(
                &mut stdin,
                &mut reader,
                3,
                &run_raw_name,
                serde_json::json!({ "argv": ["one", "two"] }),
            )
        };

        assert_eq!(call_resp["result"]["isError"], false);
        if let Some(expected) = expected_stdout {
            assert_eq!(call_resp["result"]["structuredContent"]["stdout"], expected);
        }

        drop(stdin);
        let status = child.wait().expect("wait server");
        assert!(status.success());
    }
}
