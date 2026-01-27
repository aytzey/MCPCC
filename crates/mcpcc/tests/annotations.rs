use std::path::{Path, PathBuf};
use std::process::Command;
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

fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write file");
    path
}

#[test]
fn generates_structured_tool_from_annotations_only() {
    let td = TempDir::new("mcpcc-annot-only");
    let cache_dir = td.path.join("cache");

    write_file(
        &td.path,
        "mcpcc_annot.h",
        include_str!("../../../mcpcc_annot.h"),
    );
    write_file(
        &td.path,
        "cli.c",
        r#"
#include "mcpcc_annot.h"

MCPCC_TOOL_JSON("{\"name\":\"cli\",\"description\":\"Annotated tool\",\"timeoutMs\":12345,\"maxStdoutBytes\":111,\"maxStderrBytes\":222}");

MCPCC_PARAM_JSON("{\"tool\":\"cli\",\"property\":\"verbose\",\"long\":\"--verbose\",\"short\":\"-v\",\"description\":\"More logs\",\"type\":\"boolean\"}");
MCPCC_PARAM_JSON("{\"tool\":\"cli\",\"property\":\"output\",\"long\":\"--output\",\"short\":\"-o\",\"takesValue\":true,\"required\":true,\"description\":\"Output file\",\"type\":\"string\"}");

int main(void) { return 0; }
"#,
    );

    std::fs::create_dir_all(td.path.join("bin")).expect("create bin dir");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("MCPCC_OPENROUTER_BASE_URL")
        .arg("--mcpcc-cc")
        .arg("/usr/bin/cc")
        .arg("--mcpcc-llm-mode")
        .arg("best-effort")
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--")
        .arg("cli.c")
        .arg("-o")
        .arg("bin/cli")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read(td.path.join("bin/cli.mcp.json")).expect("read mcp json");
    let v: serde_json::Value = serde_json::from_slice(&contents).expect("parse json");

    let tools = v
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");

    let structured = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("cli"))
        .expect("structured tool cli");

    assert_eq!(v["binary"]["defaultCwd"], serde_json::Value::Null);
    assert_eq!(
        structured.get("title").and_then(|v| v.as_str()),
        Some("Run cli")
    );
    assert!(
        structured
            .get("outputSchema")
            .and_then(|v| v.as_object())
            .is_some(),
        "structured tool must have outputSchema"
    );
    assert_eq!(
        structured
            .get("x-mcpcc")
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str()),
        Some("structured")
    );

    assert_eq!(
        structured.get("description").and_then(|v| v.as_str()),
        Some("Annotated tool")
    );

    assert_eq!(
        structured
            .get("x-mcpcc")
            .and_then(|v| v.get("exec"))
            .and_then(|v| v.get("timeoutMs"))
            .and_then(|v| v.as_i64()),
        Some(12345)
    );
    assert_eq!(
        structured
            .get("x-mcpcc")
            .and_then(|v| v.get("exec"))
            .and_then(|v| v.get("maxStdoutBytes"))
            .and_then(|v| v.as_i64()),
        Some(111)
    );
    assert_eq!(
        structured
            .get("x-mcpcc")
            .and_then(|v| v.get("exec"))
            .and_then(|v| v.get("maxStderrBytes"))
            .and_then(|v| v.as_i64()),
        Some(222)
    );

    let props = structured
        .get("inputSchema")
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object())
        .expect("inputSchema.properties object");

    assert_eq!(
        props
            .get("verbose")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("boolean")
    );
    assert_eq!(
        props
            .get("verbose")
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str()),
        Some("More logs")
    );

    assert_eq!(
        props
            .get("output")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("string")
    );
    assert_eq!(
        props
            .get("output")
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str()),
        Some("Output file")
    );

    let required = structured
        .get("inputSchema")
        .and_then(|v| v.get("required"))
        .and_then(|v| v.as_array())
        .expect("inputSchema.required array");
    assert!(
        required.iter().any(|v| v.as_str() == Some("output")),
        "output must be required"
    );

    let opts = structured
        .get("x-mcpcc")
        .and_then(|v| v.get("argvMapping"))
        .and_then(|v| v.get("options"))
        .and_then(|v| v.as_array())
        .expect("x-mcpcc.argvMapping.options array");
    assert_eq!(opts.len(), 2, "expected 2 annotated options");

    let verbose = opts
        .iter()
        .find(|o| o.get("param").and_then(|v| v.as_str()) == Some("verbose"))
        .expect("verbose option");
    assert_eq!(
        verbose.get("long").and_then(|v| v.as_str()),
        Some("--verbose")
    );
    assert_eq!(verbose.get("short").and_then(|v| v.as_str()), Some("-v"));
    assert_eq!(verbose.get("arg").and_then(|v| v.as_str()), Some("none"));

    let output = opts
        .iter()
        .find(|o| o.get("param").and_then(|v| v.as_str()) == Some("output"))
        .expect("output option");
    assert_eq!(
        output.get("long").and_then(|v| v.as_str()),
        Some("--output")
    );
    assert_eq!(output.get("short").and_then(|v| v.as_str()), Some("-o"));
    assert_eq!(output.get("arg").and_then(|v| v.as_str()), Some("required"));

    let run_raw = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("cli.run_raw"))
        .expect("fallback tool cli.run_raw");
    assert_eq!(
        run_raw.get("title").and_then(|v| v.as_str()),
        Some("Run cli (raw argv)")
    );
    assert!(
        run_raw
            .get("outputSchema")
            .and_then(|v| v.as_object())
            .is_some(),
        "run_raw tool must have outputSchema"
    );
    assert_eq!(
        run_raw
            .get("x-mcpcc")
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str()),
        Some("run_raw")
    );
    let exec = run_raw
        .get("x-mcpcc")
        .and_then(|v| v.get("exec"))
        .and_then(|v| v.as_object())
        .expect("fallback tool must have x-mcpcc.exec object");
    assert_eq!(exec.get("timeoutMs").and_then(|v| v.as_i64()), Some(30000));
    assert_eq!(
        exec.get("maxStdoutBytes").and_then(|v| v.as_i64()),
        Some(1048576)
    );
    assert_eq!(
        exec.get("maxStderrBytes").and_then(|v| v.as_i64()),
        Some(1048576)
    );
}

#[test]
fn annotations_override_argp_extractor() {
    let td = TempDir::new("mcpcc-annot-argp");
    let cache_dir = td.path.join("cache");

    write_file(
        &td.path,
        "mcpcc_annot.h",
        include_str!("../../../mcpcc_annot.h"),
    );
    write_file(
        &td.path,
        "cli.c",
        r#"
#include <argp.h>
#include "mcpcc_annot.h"

MCPCC_TOOL_JSON("{\"name\":\"cli\",\"description\":\"Overridden desc\"}");
MCPCC_PARAM_JSON("{\"tool\":\"cli\",\"property\":\"help\",\"long\":\"--assist\",\"short\":\"-x\",\"description\":\"Overridden help\",\"type\":\"boolean\"}");

static struct argp_option options[] = {
  {"help", 'h', 0, 0, "Show help", 0},
  {0}
};

static struct argp argp = { options, 0, 0, 0 };

int main(int argc, char **argv) {
  argp_parse(&argp, argc, argv, 0, 0, 0);
  return 0;
}
"#,
    );

    std::fs::create_dir_all(td.path.join("bin")).expect("create bin dir");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("MCPCC_OPENROUTER_BASE_URL")
        .arg("--mcpcc-cc")
        .arg("/usr/bin/cc")
        .arg("--mcpcc-llm-mode")
        .arg("best-effort")
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--")
        .arg("cli.c")
        .arg("-o")
        .arg("bin/cli")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read(td.path.join("bin/cli.mcp.json")).expect("read mcp json");
    let v: serde_json::Value = serde_json::from_slice(&contents).expect("parse json");

    let tools = v
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");

    let structured = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("cli"))
        .expect("structured tool cli");
    let exec = structured
        .get("x-mcpcc")
        .and_then(|v| v.get("exec"))
        .and_then(|v| v.as_object())
        .expect("structured tool must have x-mcpcc.exec object");
    assert_eq!(exec.get("timeoutMs").and_then(|v| v.as_i64()), Some(30000));
    assert_eq!(
        exec.get("maxStdoutBytes").and_then(|v| v.as_i64()),
        Some(1048576)
    );
    assert_eq!(
        exec.get("maxStderrBytes").and_then(|v| v.as_i64()),
        Some(1048576)
    );

    assert_eq!(
        structured.get("description").and_then(|v| v.as_str()),
        Some("Overridden desc")
    );

    let props = structured
        .get("inputSchema")
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object())
        .expect("inputSchema.properties object");
    assert_eq!(
        props
            .get("help")
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str()),
        Some("Overridden help")
    );

    let opts = structured
        .get("x-mcpcc")
        .and_then(|v| v.get("argvMapping"))
        .and_then(|v| v.get("options"))
        .and_then(|v| v.as_array())
        .expect("x-mcpcc.argvMapping.options array");
    let help = opts
        .iter()
        .find(|o| o.get("param").and_then(|v| v.as_str()) == Some("help"))
        .expect("help option");
    assert_eq!(help.get("long").and_then(|v| v.as_str()), Some("--assist"));
    assert_eq!(help.get("short").and_then(|v| v.as_str()), Some("-x"));
}
