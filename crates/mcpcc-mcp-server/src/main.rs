use std::collections::HashSet;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct McpJsonBundle {
    mcp_spec_version: String,
    binary_path: String,
    tools: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy)]
struct ExecLimits {
    timeout_ms: u64,
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    AwaitingInitialize,
    AwaitingInitialized,
    Ready,
}

#[derive(Debug)]
enum ServerError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidMcpJson(String),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerError::Io(err) => write!(f, "{err}"),
            ServerError::Json(err) => write!(f, "{err}"),
            ServerError::InvalidMcpJson(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<std::io::Error> for ServerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ServerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("mcpcc-mcp-server: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), ServerError> {
    let mcp_json_path = resolve_adjacent_mcp_json_path()?;
    let bundle = load_mcp_json_bundle(&mcp_json_path)?;
    run_stdio_server(&bundle)
}

fn resolve_adjacent_mcp_json_path() -> Result<PathBuf, ServerError> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let file_name = exe.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
        ServerError::InvalidMcpJson("server exe filename is not valid UTF-8".into())
    })?;

    let base = file_name.strip_suffix(".mcp-server").unwrap_or(file_name);
    Ok(dir.join(format!("{base}.mcp.json")))
}

fn load_mcp_json_bundle(path: &Path) -> Result<McpJsonBundle, ServerError> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ServerError::InvalidMcpJson(format!(
                "missing MCP bundle JSON: {}",
                path.to_string_lossy()
            ))
        } else {
            ServerError::Io(err)
        }
    })?;

    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let obj = v
        .as_object()
        .ok_or_else(|| ServerError::InvalidMcpJson("mcp.json must be a JSON object".into()))?;

    let mcp_spec_version = obj
        .get("mcpSpecVersion")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            ServerError::InvalidMcpJson(
                "mcp.json missing required string field: mcpSpecVersion".into(),
            )
        })?;

    let binary_path = obj
        .get("binary")
        .and_then(|v| v.as_object())
        .and_then(|binary| binary.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            ServerError::InvalidMcpJson(
                "mcp.json missing required string field: binary.path".into(),
            )
        })?;

    let tools = obj
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ServerError::InvalidMcpJson("mcp.json missing required array field: tools".into())
        })?
        .iter()
        .cloned()
        .collect::<Vec<_>>();

    if tools.is_empty() {
        return Err(ServerError::InvalidMcpJson(
            "mcp.json tools[] must be non-empty".into(),
        ));
    }

    Ok(McpJsonBundle {
        mcp_spec_version,
        binary_path,
        tools,
    })
}

fn run_stdio_server(bundle: &McpJsonBundle) -> Result<(), ServerError> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();

    let mut lifecycle = Lifecycle::AwaitingInitialize;

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(err) => {
                let resp = jsonrpc_error(serde_json::Value::Null, -32700, &err.to_string());
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(resp) = handle_message(&msg, bundle, &mut lifecycle) {
            writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_message(
    msg: &serde_json::Value,
    bundle: &McpJsonBundle,
    lifecycle: &mut Lifecycle,
) -> Option<serde_json::Value> {
    let obj = msg.as_object()?;
    let jsonrpc = obj.get("jsonrpc").and_then(|v| v.as_str());
    if jsonrpc != Some("2.0") {
        let id = obj.get("id").cloned().unwrap_or(serde_json::Value::Null);
        if obj.get("id").is_some() {
            return Some(jsonrpc_error(id, -32600, "invalid Request"));
        }
        return None;
    }

    let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = obj.get("id").cloned();
    let is_request = id.is_some();

    match method {
        "initialize" => {
            if !is_request {
                return None;
            }

            let id = id.unwrap_or(serde_json::Value::Null);
            if *lifecycle != Lifecycle::AwaitingInitialize {
                return Some(jsonrpc_error(
                    id,
                    -32600,
                    "initialize called in invalid lifecycle state",
                ));
            }

            *lifecycle = Lifecycle::AwaitingInitialized;
            let result = serde_json::json!({
                "protocolVersion": bundle.mcp_spec_version.as_str(),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": env!("CARGO_PKG_NAME"),
                    "version": env!("CARGO_PKG_VERSION"),
                },
            });
            Some(jsonrpc_result(id, result))
        }
        "initialized" => {
            if *lifecycle == Lifecycle::AwaitingInitialized {
                *lifecycle = Lifecycle::Ready;
            }
            if is_request {
                Some(jsonrpc_result(
                    id.unwrap_or(serde_json::Value::Null),
                    serde_json::Value::Null,
                ))
            } else {
                None
            }
        }
        "tools/listTools" => {
            if !is_request {
                return None;
            }
            let id = id.unwrap_or(serde_json::Value::Null);
            if *lifecycle != Lifecycle::Ready {
                return Some(jsonrpc_error(
                    id,
                    -32002,
                    "server not initialized (expected initialize then initialized)",
                ));
            }
            let result = serde_json::json!({
                "tools": bundle.tools.clone(),
            });
            Some(jsonrpc_result(id, result))
        }
        "tools/callTool" => {
            if !is_request {
                return None;
            }
            let id = id.unwrap_or(serde_json::Value::Null);
            if *lifecycle != Lifecycle::Ready {
                return Some(jsonrpc_error(
                    id,
                    -32002,
                    "server not initialized (expected initialize then initialized)",
                ));
            }

            match handle_call_tool(obj, bundle) {
                Ok(result) => Some(jsonrpc_result(id, result)),
                Err(msg) => Some(jsonrpc_error(id, -32602, &msg)),
            }
        }
        _ => {
            if !is_request {
                return None;
            }
            Some(jsonrpc_error(
                id.unwrap_or(serde_json::Value::Null),
                -32601,
                "method not found",
            ))
        }
    }
}

fn handle_call_tool(
    msg_obj: &serde_json::Map<String, serde_json::Value>,
    bundle: &McpJsonBundle,
) -> Result<serde_json::Value, String> {
    let params = msg_obj
        .get("params")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "invalid params (expected object): params".to_string())?;

    let tool_name = params
        .get("name")
        .or_else(|| params.get("toolName"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "invalid params (expected string): params.name".to_string())?;

    let Some(tool) = find_tool(bundle, tool_name) else {
        return Ok(tool_error_result(&format!("tool not found: {tool_name}")));
    };

    if tool_name.ends_with(".run_raw") {
        let args_obj = params
            .get("arguments")
            .and_then(|v| v.as_object())
            .ok_or_else(|| "invalid params (expected object): params.arguments".to_string())?;

        let argv = args_obj
            .get("argv")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "invalid params (expected array): arguments.argv".to_string())?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "invalid params (expected string): arguments.argv[]".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;

        return Ok(run_binary_tool(&bundle.binary_path, &argv, tool));
    }

    let empty_arguments = serde_json::Map::new();
    let args_obj = params
        .get("arguments")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty_arguments);

    match argv_from_structured_tool_call(tool, args_obj) {
        Ok(argv) => Ok(run_binary_tool(&bundle.binary_path, &argv, tool)),
        Err(msg) => Ok(tool_error_result(&msg)),
    }
}

fn find_tool<'a>(bundle: &'a McpJsonBundle, name: &str) -> Option<&'a serde_json::Value> {
    bundle.tools.iter().find(|tool| {
        tool.as_object()
            .and_then(|obj| obj.get("name"))
            .and_then(|v| v.as_str())
            == Some(name)
    })
}

fn run_binary_tool(
    binary_path: &str,
    argv: &[String],
    tool: &serde_json::Value,
) -> serde_json::Value {
    let limits = tool_exec_limits(tool);
    let outcome = run_raw_binary(binary_path, argv, limits);

    let summary = if let Some(err) = &outcome.spawn_error {
        format!("spawn error: {err}")
    } else if outcome.timed_out {
        format!("exitCode={} timedOut=true", outcome.exit_code)
    } else {
        format!("exitCode={}", outcome.exit_code)
    };

    serde_json::json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": {
            "stdout": String::from_utf8_lossy(&outcome.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&outcome.stderr).to_string(),
            "exitCode": outcome.exit_code,
            "durationMs": outcome.duration_ms,
            "timedOut": outcome.timed_out,
            "truncatedStdout": outcome.truncated_stdout,
            "truncatedStderr": outcome.truncated_stderr,
        },
        "isError": outcome.is_error,
    })
}

fn argv_from_structured_tool_call(
    tool: &serde_json::Value,
    arguments: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<String>, String> {
    let enforce_additional_properties = tool
        .get("inputSchema")
        .and_then(|v| v.get("additionalProperties"))
        .and_then(|v| v.as_bool())
        == Some(false);

    if enforce_additional_properties {
        let Some(props) = tool
            .get("inputSchema")
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.as_object())
        else {
            return Err(
                "tool inputSchema.properties missing (cannot validate additionalProperties:false)"
                    .to_string(),
            );
        };

        let allowed: HashSet<&str> = props.keys().map(String::as_str).collect();
        for key in arguments.keys() {
            if !allowed.contains(key.as_str()) {
                return Err(format!("unknown argument property: {key}"));
            }
        }
    }

    if let Some(required) = tool
        .get("inputSchema")
        .and_then(|v| v.get("required"))
        .and_then(|v| v.as_array())
    {
        for entry in required {
            let Some(key) = entry.as_str() else {
                continue;
            };
            if !arguments.contains_key(key) {
                return Err(format!("missing required argument property: {key}"));
            }
        }
    }

    let mapping = tool
        .get("x-mcpcc")
        .and_then(|v| v.get("argvMapping"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| "tool missing x-mcpcc.argvMapping".to_string())?;

    let options = mapping
        .get("options")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "tool missing x-mcpcc.argvMapping.options".to_string())?;

    let args_param = mapping
        .get("positionalProperty")
        .and_then(|v| v.as_str())
        .or_else(|| mapping.get("argsParam").and_then(|v| v.as_str()))
        .unwrap_or("args");

    let mut argv = Vec::new();

    for opt in options {
        let Some(opt_obj) = opt.as_object() else {
            return Err("invalid x-mcpcc.argvMapping.options entry (expected object)".to_string());
        };

        let property = opt_obj
            .get("property")
            .or_else(|| opt_obj.get("param"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                "invalid x-mcpcc.argvMapping.options entry: missing property".to_string()
            })?;

        let Some(value) = arguments.get(property) else {
            continue;
        };

        let repeatable = opt_obj
            .get("repeatable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let flag = if let Some(long) = opt_obj.get("long").and_then(|v| v.as_str()) {
            long
        } else if let Some(short) = opt_obj.get("short").and_then(|v| v.as_str()) {
            short
        } else {
            return Err(format!(
                "invalid x-mcpcc.argvMapping.options entry for {property}: missing long/short"
            ));
        };

        let arg_requirement = if let Some(arg) = opt_obj.get("arg").and_then(|v| v.as_str()) {
            arg
        } else if let Some(takes_value) = opt_obj.get("takesValue").and_then(|v| v.as_bool()) {
            if takes_value {
                "optional"
            } else {
                "none"
            }
        } else if value.is_boolean() {
            "none"
        } else {
            "optional"
        };

        if repeatable {
            if let Some(values) = value.as_array() {
                for entry in values {
                    apply_option_value(&mut argv, property, flag, arg_requirement, entry)?;
                }
            } else {
                apply_option_value(&mut argv, property, flag, arg_requirement, value)?;
            }
        } else {
            apply_option_value(&mut argv, property, flag, arg_requirement, value)?;
        }
    }

    if let Some(value) = arguments.get(args_param) {
        let Some(values) = value.as_array() else {
            return Err(format!(
                "invalid arguments (expected array): arguments.{args_param}"
            ));
        };
        for entry in values {
            let Some(arg) = entry.as_str() else {
                return Err(format!(
                    "invalid arguments (expected string): arguments.{args_param}[]"
                ));
            };
            argv.push(arg.to_string());
        }
    }

    Ok(argv)
}

fn apply_option_value(
    argv: &mut Vec<String>,
    param: &str,
    flag: &str,
    arg_requirement: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    match arg_requirement {
        "none" => {
            let Some(enabled) = value.as_bool() else {
                return Err(format!(
                    "invalid arguments (expected boolean): arguments.{param}"
                ));
            };
            if enabled {
                argv.push(flag.to_string());
            }
            Ok(())
        }
        "required" => {
            let Some(arg) = value.as_str() else {
                return Err(format!(
                    "invalid arguments (expected string): arguments.{param}"
                ));
            };
            argv.push(flag.to_string());
            argv.push(arg.to_string());
            Ok(())
        }
        "optional" => {
            let Some(arg) = value.as_str() else {
                return Err(format!(
                    "invalid arguments (expected string): arguments.{param}"
                ));
            };
            argv.push(flag.to_string());
            if !arg.is_empty() {
                argv.push(arg.to_string());
            }
            Ok(())
        }
        other => Err(format!(
            "invalid x-mcpcc.argvMapping.options entry for {param}: unsupported arg requirement {other}"
        )),
    }
}

fn tool_exec_limits(tool: &serde_json::Value) -> ExecLimits {
    const DEFAULT_TIMEOUT_MS: u64 = 30_000;
    const DEFAULT_MAX_STDOUT_BYTES: usize = 1_048_576;
    const DEFAULT_MAX_STDERR_BYTES: usize = 1_048_576;

    let mut limits = ExecLimits {
        timeout_ms: DEFAULT_TIMEOUT_MS,
        max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
        max_stderr_bytes: DEFAULT_MAX_STDERR_BYTES,
    };

    let Some(exec) = tool
        .get("x-mcpcc")
        .and_then(|v| v.as_object())
        .and_then(|x| x.get("exec"))
        .and_then(|v| v.as_object())
    else {
        return limits;
    };

    if let Some(timeout_ms) = exec.get("timeoutMs").and_then(|v| v.as_u64()) {
        limits.timeout_ms = timeout_ms;
    }
    if let Some(max_stdout) = exec
        .get("maxStdoutBytes")
        .and_then(|v| v.as_u64())
        .and_then(|v| usize::try_from(v).ok())
    {
        limits.max_stdout_bytes = max_stdout;
    }
    if let Some(max_stderr) = exec
        .get("maxStderrBytes")
        .and_then(|v| v.as_u64())
        .and_then(|v| usize::try_from(v).ok())
    {
        limits.max_stderr_bytes = max_stderr;
    }

    limits
}

#[derive(Debug)]
struct ToolRunOutcome {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i64,
    duration_ms: u64,
    timed_out: bool,
    truncated_stdout: bool,
    truncated_stderr: bool,
    is_error: bool,
    spawn_error: Option<String>,
}

fn run_raw_binary(binary_path: &str, argv: &[String], limits: ExecLimits) -> ToolRunOutcome {
    let started = Instant::now();
    let mut cmd = std::process::Command::new(binary_path);
    cmd.args(argv)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ToolRunOutcome {
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit_code: -1,
                duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                timed_out: false,
                truncated_stdout: false,
                truncated_stderr: false,
                is_error: true,
                spawn_error: Some(err.to_string()),
            };
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let stdout_thread = std::thread::spawn(move || read_limited(stdout, limits.max_stdout_bytes));
    let stderr_thread = std::thread::spawn(move || read_limited(stderr, limits.max_stderr_bytes));

    let timeout = Duration::from_millis(limits.timeout_ms);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait();
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(err);
            }
        }
    };

    let (stdout, truncated_stdout) = stdout_thread.join().unwrap_or_else(|_| (Vec::new(), false));
    let (stderr, truncated_stderr) = stderr_thread.join().unwrap_or_else(|_| (Vec::new(), false));

    let exit_code = match status.as_ref() {
        Ok(status) => exit_status_code(status),
        Err(_) => -1,
    };
    let duration_ms: u64 = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let is_error = timed_out || exit_code != 0;

    ToolRunOutcome {
        stdout,
        stderr,
        exit_code,
        duration_ms,
        timed_out,
        truncated_stdout,
        truncated_stderr,
        is_error,
        spawn_error: None,
    }
}

fn exit_status_code(status: &std::process::ExitStatus) -> i64 {
    if let Some(code) = status.code() {
        return i64::from(code);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return i64::from(128 + signal);
        }
    }

    -1
}

fn tool_error_result(message: &str) -> serde_json::Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": message }],
        "structuredContent": {
            "stdout": "",
            "stderr": message,
            "exitCode": -1,
            "durationMs": 0,
            "timedOut": false,
            "truncatedStdout": false,
            "truncatedStderr": false,
        },
        "isError": true,
    })
}

fn read_limited<R: Read>(mut reader: R, limit: usize) -> (Vec<u8>, bool) {
    let mut buf = [0u8; 4096];
    let mut collected = Vec::new();
    let mut truncated = false;

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        let remaining = limit.saturating_sub(collected.len());
        if remaining == 0 {
            truncated = true;
            continue;
        }

        let to_copy = remaining.min(n);
        collected.extend_from_slice(&buf[..to_copy]);
        if to_copy < n {
            truncated = true;
        }
    }

    (collected, truncated)
}

fn jsonrpc_result(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}
