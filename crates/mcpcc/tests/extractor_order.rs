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
fn prefers_argp_over_getopt_long_when_both_detected() {
    let td = TempDir::new("mcpcc-extractor-order");
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

    std::fs::write(
        td.path.join("cli.c"),
        r#"
#include <argp.h>
#include <getopt.h>

static struct argp_option options[] = {
  {"argp_only", 'a', "VAL", 0, "Argp only", 0},
  {0}
};

static struct argp argp = { options, 0, 0, 0 };

static struct option long_options[] = {
  {"getopt_only", no_argument, 0, 'g'},
  {0, 0, 0, 0},
};

int main(int argc, char **argv) {
  int opt;
  argp_parse(&argp, argc, argv, 0, 0, 0);
  while ((opt = getopt_long(argc, argv, "ag", long_options, 0)) != -1) {
    (void)opt;
  }
  return 0;
}
"#,
    )
    .expect("write cli.c");

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
    let mcp_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&mcp_json_path).expect("read mcp json"))
            .expect("parse mcp json");

    let tools = mcp_json
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");
    let structured = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("cli"))
        .expect("structured tool");
    let props = structured
        .get("inputSchema")
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object())
        .expect("inputSchema.properties");

    assert!(
        props.contains_key("argp_only"),
        "expected argp_only property"
    );
    assert!(
        !props.contains_key("getopt_only"),
        "getopt_long should not win over argp"
    );
}
