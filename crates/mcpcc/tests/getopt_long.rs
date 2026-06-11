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

fn write_exe(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write exe");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
    }

    path
}

#[test]
fn generates_structured_tool_from_pj_getopt_long() {
    let td = TempDir::new("mcpcc-pj-getopt-long");
    let cache_dir = td.path.join("cache");

    let cc_path = write_exe(
        &td.path,
        "fakecc",
        br#"#!/bin/sh
set -eu
out="a.out"
prev=""
for a in "$@"; do
  if [ "$prev" = "-o" ]; then
    out="$a"
    prev=""
    continue
  fi
  case "$a" in
    -o) prev="-o" ;;
    -o*) out="${a#-o}" ;;
  esac
done
mkdir -p "$(dirname "$out")"
: > "$out"
exit 0
"#,
    );

    // Mirrors pjsua's CLI parsing: PJLIB's own getopt reimplementation with
    // numeric has_arg values, enum (non-char) `val` fields, an empty
    // optstring, and a duplicated entry.
    let source = br#"
#include <pjlib-util.h>

enum { OPT_CONFIG_FILE, OPT_LOG_LEVEL, OPT_LOG_APPEND };

static int parse_args(int argc, char *argv[]) {
  struct pj_getopt_option long_options[] = {
    { "config-file", 1, 0, OPT_CONFIG_FILE},
    { "log-level",   1, 0, OPT_LOG_LEVEL},
    { "log-append",  0, 0, OPT_LOG_APPEND},
    { "log-level",   1, 0, OPT_LOG_LEVEL},
    { NULL, 0, 0, 0}
  };
  int c, option_index;
  pj_optind = 0;
  while ((c = pj_getopt_long(argc, argv, "", long_options, &option_index)) != -1) {
    (void)c;
  }
  return 0;
}

int main(int argc, char *argv[]) { return parse_args(argc, argv); }
"#;
    std::fs::write(td.path.join("pjcli.c"), source).expect("write pjcli.c");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("MCPCC_OPENROUTER_BASE_URL")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--mcpcc-llm-mode")
        .arg("best-effort")
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--")
        .arg("pjcli.c")
        .arg("-o")
        .arg("bin/pjcli")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read(td.path.join("bin/pjcli.mcp.json")).expect("read mcp json");
    let v: serde_json::Value = serde_json::from_slice(&contents).expect("parse json");
    let tools = v
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");
    let structured = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("pjcli"))
        .expect("structured tool pjcli");

    let opts = structured
        .get("x-mcpcc")
        .and_then(|v| v.get("argvMapping"))
        .and_then(|v| v.get("options"))
        .and_then(|v| v.as_array())
        .expect("argvMapping.options array");
    let names: Vec<&str> = opts
        .iter()
        .filter_map(|o| o.get("property").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(
        names,
        ["config-file", "log-level", "log-append"],
        "duplicate long names must be deduped, order preserved"
    );
    assert_eq!(
        opts[0].get("arg").and_then(|v| v.as_str()),
        Some("required"),
        "numeric has_arg=1 must map to required"
    );
    assert_eq!(
        opts[2].get("arg").and_then(|v| v.as_str()),
        Some("none"),
        "numeric has_arg=0 must map to a flag"
    );
}

#[test]
fn generates_structured_tool_from_getopt_long() {
    let td = TempDir::new("mcpcc-getopt-long");
    let cache_dir = td.path.join("cache");

    let cc_path = write_exe(
        &td.path,
        "fakecc",
        br#"#!/bin/sh
set -eu
out="a.out"
prev=""
for a in "$@"; do
  if [ "$prev" = "-o" ]; then
    out="$a"
    prev=""
    continue
  fi
  case "$a" in
    -o)
      prev="-o"
      ;;
    -o*)
      out="${a#-o}"
      ;;
  esac
done
mkdir -p "$(dirname "$out")"
: > "$out"
exit 0
"#,
    );

    let source = br#"
#include <getopt.h>

static struct option long_options[] = {
  {"help", no_argument, 0, 'h'},
  {"output", required_argument, 0, 'o'},
  {"color", optional_argument, 0, 'c'},
  {0, 0, 0, 0},
};

int main(int argc, char **argv) {
  int opt;
  while ((opt = getopt_long(argc, argv, "ho:c::", long_options, 0)) != -1) {
    (void)opt;
  }
  (void)argc;
  (void)argv;
  return 0;
}
"#;
    std::fs::write(td.path.join("cli.c"), source).expect("write cli.c");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("MCPCC_OPENROUTER_BASE_URL")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
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

    let mcp_json_path = td.path.join("bin/cli.mcp.json");
    let contents = std::fs::read(&mcp_json_path).expect("read mcp json");
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
        Some("cli")
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
        structured
            .get("inputSchema")
            .and_then(|v| v.get("additionalProperties"))
            .and_then(|v| v.as_bool()),
        Some(false),
        "inputSchema.additionalProperties must be false"
    );

    let props = structured
        .get("inputSchema")
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object())
        .expect("inputSchema.properties object");

    assert_eq!(
        props
            .get("help")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("boolean")
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
            .get("color")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("string")
    );
    assert_eq!(
        props
            .get("args")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("array")
    );
    assert_eq!(
        props
            .get("args")
            .and_then(|v| v.get("items"))
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("string")
    );

    let opts = structured
        .get("x-mcpcc")
        .and_then(|v| v.get("argvMapping"))
        .and_then(|v| v.get("options"))
        .and_then(|v| v.as_array())
        .expect("x-mcpcc.argvMapping.options array");
    assert_eq!(opts.len(), 3, "expected 3 extracted options");

    assert_eq!(
        opts[0].get("property").and_then(|v| v.as_str()),
        Some("help")
    );
    assert_eq!(
        opts[0].get("takesValue").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(opts[0].get("arg").and_then(|v| v.as_str()), Some("none"));
    assert_eq!(
        opts[0].get("valueStyle").and_then(|v| v.as_str()),
        Some("separate")
    );
    assert_eq!(
        opts[0].get("repeatable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(opts[0].get("position").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(opts[0].get("short").and_then(|v| v.as_str()), Some("-h"));

    assert_eq!(
        opts[1].get("property").and_then(|v| v.as_str()),
        Some("output")
    );
    assert_eq!(
        opts[1].get("takesValue").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        opts[1].get("arg").and_then(|v| v.as_str()),
        Some("required")
    );
    assert_eq!(
        opts[1].get("valueStyle").and_then(|v| v.as_str()),
        Some("separate")
    );
    assert_eq!(
        opts[1].get("repeatable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(opts[1].get("position").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(opts[1].get("short").and_then(|v| v.as_str()), Some("-o"));

    assert_eq!(
        opts[2].get("property").and_then(|v| v.as_str()),
        Some("color")
    );
    assert_eq!(
        opts[2].get("takesValue").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        opts[2].get("arg").and_then(|v| v.as_str()),
        Some("optional")
    );
    // Optional-argument values must serialize attached (`--color=WHEN`);
    // GNU getopt_long treats a separate token as a positional instead.
    assert_eq!(
        opts[2].get("valueStyle").and_then(|v| v.as_str()),
        Some("attached")
    );
    assert_eq!(
        opts[2].get("repeatable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(opts[2].get("position").and_then(|v| v.as_i64()), Some(2));
    assert_eq!(opts[2].get("short").and_then(|v| v.as_str()), Some("-c"));

    assert_eq!(
        structured
            .get("x-mcpcc")
            .and_then(|v| v.get("argvMapping"))
            .and_then(|v| v.get("positionalProperty"))
            .and_then(|v| v.as_str()),
        Some("args")
    );

    let run_raw = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("cli.run_raw"))
        .expect("fallback tool cli.run_raw");
    assert_eq!(
        run_raw.get("title").and_then(|v| v.as_str()),
        Some("cli.run_raw")
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
        Some("raw")
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
