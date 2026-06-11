//! CLI surface promised by the PRD: `--mcpcc-help`, `--mcpcc-version`, and the
//! `MCPCC_*` environment variable fallbacks.

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

fn link_fake_compiler(dir: &Path) -> PathBuf {
    write_exe(
        dir,
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
    )
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .arg("--mcpcc-help")
        .output()
        .expect("run mcpcc");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("USAGE"), "missing USAGE in: {stdout}");
    assert!(
        stdout.contains("--mcpcc-llm-mode"),
        "missing flags: {stdout}"
    );
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .arg("--mcpcc-version")
        .output()
        .expect("run mcpcc");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim().starts_with("mcpcc "),
        "unexpected version output: {stdout}"
    );
}

#[test]
fn invalid_mcpcc_llm_mode_env_fails_with_usage_error() {
    let td = TempDir::new("mcpcc-env-llm-mode");
    let cc_path = link_fake_compiler(&td.path);

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env("MCPCC_LLM_MODE", "bogus")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--")
        .arg("hello.c")
        .arg("-o")
        .arg("hello")
        .output()
        .expect("run mcpcc");

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid MCPCC_LLM_MODE"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn env_vars_drive_llm_mode_and_artifacts_dir() {
    let td = TempDir::new("mcpcc-env-dirs");
    let cc_path = link_fake_compiler(&td.path);
    let artifacts_dir = td.path.join("env-artifacts");
    let cache_dir = td.path.join("env-cache");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env("MCPCC_LLM_MODE", "best-effort")
        .env("MCPCC_ARTIFACTS_DIR", &artifacts_dir)
        .env("MCPCC_CACHE_DIR", &cache_dir)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--")
        .arg("hello.c")
        .arg("-o")
        .arg("bin/hello")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        artifacts_dir.join("hello.mcp.json").is_file(),
        "mcp.json should land in MCPCC_ARTIFACTS_DIR"
    );
    assert!(
        artifacts_dir.join("hello.mcp-server").is_file(),
        "server should land in MCPCC_ARTIFACTS_DIR"
    );
    assert!(
        artifacts_dir.join("hello.mcpcc-manifest.json").is_file(),
        "manifest should land in MCPCC_ARTIFACTS_DIR"
    );
}

#[test]
fn cc_pointing_at_mcpcc_itself_fails_instead_of_forking_forever() {
    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .env("CC", bin)
        .arg("--mcpcc-print-cc")
        .output()
        .expect("run mcpcc");

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("refusing to use mcpcc itself"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn response_file_link_line_still_produces_artifacts() {
    let td = TempDir::new("mcpcc-response-file");
    let cc_path = link_fake_compiler(&td.path);

    // CMake+Ninja style: the whole link line lives in an @rsp file.
    std::fs::write(td.path.join("link.rsp"), "hello.c -o bin/hello\n").expect("write rsp");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env("MCPCC_ALLOW_NO_LLM", "1")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--mcpcc-llm-mode")
        .arg("off")
        .arg("--")
        .arg("@link.rsp")
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        td.path.join("bin/hello.mcp.json").is_file(),
        "expected artifacts for -o found inside the response file"
    );
}
