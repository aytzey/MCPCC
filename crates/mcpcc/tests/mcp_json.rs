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
fn writes_minimal_mcp_json_on_successful_link() {
    let td = TempDir::new("mcpcc-mcp-json-link");
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
        .arg("hello.c")
        .arg("-o")
        .arg("bin/hello")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let mcp_json_path = td.path.join("bin/hello.mcp.json");
    let contents = std::fs::read(&mcp_json_path).expect("read mcp json");
    let v: serde_json::Value = serde_json::from_slice(&contents).expect("parse json");

    assert_eq!(
        v.get("mcpccVersion").and_then(|v| v.as_str()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        v.get("mcpSpecVersion").and_then(|v| v.as_str()),
        Some(mcpcc::MCP_SPEC_VERSION)
    );
    assert_eq!(
        v.get("binary")
            .and_then(|b| b.get("path"))
            .and_then(|p| p.as_str()),
        Some("bin/hello")
    );

    let tools = v
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");
    assert!(!tools.is_empty(), "tools[] must be non-empty");

    let tool = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("hello.run_raw"))
        .expect("fallback tool hello.run_raw");
    assert!(
        tool.get("description").and_then(|v| v.as_str()).is_some(),
        "fallback tool must have a description"
    );
    assert!(
        tool.get("inputSchema")
            .and_then(|v| v.as_object())
            .is_some(),
        "fallback tool must have a non-null inputSchema object"
    );

    for entry in std::fs::read_dir(td.path.join("bin")).expect("read dir") {
        let entry = entry.expect("entry");
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".mcpcc-tmp-"),
            "temp file should not remain: {name}"
        );
    }

    let manifest_path = td.path.join("bin/hello.mcpcc-manifest.json");
    let contents = std::fs::read(&manifest_path).expect("read manifest");
    let v: serde_json::Value = serde_json::from_slice(&contents).expect("parse manifest json");

    assert_eq!(
        v.get("binary")
            .and_then(|b| b.get("path"))
            .and_then(|p| p.as_str()),
        Some("bin/hello")
    );
    assert_eq!(
        v.get("compiler")
            .and_then(|c| c.get("exitCode"))
            .and_then(|v| v.as_i64()),
        Some(0)
    );
    assert!(
        v.get("compiler")
            .and_then(|c| c.get("argv"))
            .and_then(|v| v.as_array())
            .is_some(),
        "manifest must include compiler.argv array"
    );
    assert_eq!(
        v.get("artifacts")
            .and_then(|a| a.get("mcpJson"))
            .and_then(|v| v.as_str()),
        Some("bin/hello.mcp.json")
    );
    assert!(
        v.get("analysis").and_then(|v| v.as_object()).is_some(),
        "manifest must include analysis object"
    );
    let llm = v
        .get("llm")
        .and_then(|v| v.as_object())
        .expect("llm object");
    assert_eq!(
        llm.get("mode").and_then(|v| v.as_str()),
        Some("best-effort")
    );
    assert_eq!(
        llm.get("provider").and_then(|v| v.as_str()),
        Some("openrouter")
    );
    assert_eq!(llm.get("cacheHit").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        llm.get("promptVersion").and_then(|v| v.as_str()),
        Some(mcpcc::LLM_PROMPT_VERSION)
    );
    assert_eq!(
        llm.get("usedPlaceholder").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(
        llm.get("error").and_then(|v| v.as_str()).is_some(),
        "manifest must record placeholder reason"
    );
}

#[test]
fn does_not_write_mcp_json_for_compile_only_invocations() {
    let td = TempDir::new("mcpcc-mcp-json-compile-only");
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

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--")
        .arg("-c")
        .arg("hello.c")
        .arg("-o")
        .arg("obj/hello.o")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !td.path.join("obj/hello.o.mcp.json").exists(),
        "compile-only mode must not produce mcp.json"
    );
    assert!(
        !td.path.join("obj/hello.o.mcpcc-manifest.json").exists(),
        "compile-only mode must not produce manifest"
    );
}

#[test]
fn does_not_write_mcp_json_when_compiler_fails() {
    let td = TempDir::new("mcpcc-mcp-json-compiler-fails");
    let cc_path = write_exe(&td.path, "fakecc", b"#!/bin/sh\nexit 7\n");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--")
        .arg("hello.c")
        .arg("-o")
        .arg("bin/hello")
        .output()
        .expect("run mcpcc");

    assert_eq!(
        out.status.code(),
        Some(7),
        "expected exit 7, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !td.path.join("bin/hello.mcp.json").exists(),
        "non-zero compiler exit must not produce mcp.json"
    );
    assert!(
        !td.path.join("bin/hello.mcpcc-manifest.json").exists(),
        "non-zero compiler exit must not produce manifest"
    );
}

#[test]
fn mcp_generation_failure_exits_70() {
    let td = TempDir::new("mcpcc-mcp-gen-fails");
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
        .arg("--mcpcc-mcp-json-out")
        .arg("bin")
        .arg("--")
        .arg("hello.c")
        .arg("-o")
        .arg("bin/hello")
        .output()
        .expect("run mcpcc");

    assert_eq!(
        out.status.code(),
        Some(70),
        "expected exit 70, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !td.path.join("bin/hello.mcp.json").exists(),
        "failed MCP generation must not produce mcp.json"
    );
    assert!(
        !td.path.join("bin/hello.mcpcc-manifest.json").exists(),
        "failed MCP generation must not produce manifest"
    );
    assert!(
        td.path.join("bin").is_dir(),
        "mcp generation should not clobber existing directories"
    );
}
