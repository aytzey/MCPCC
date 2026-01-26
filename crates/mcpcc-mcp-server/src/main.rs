use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct McpJsonBundle {
    mcp_spec_version: String,
    tools: Vec<serde_json::Value>,
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
