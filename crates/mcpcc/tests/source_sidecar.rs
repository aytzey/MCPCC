//! Autoconf/make-style flow: separate `-c` compile steps followed by a link
//! whose object names carry no source hint. mcpcc records `<obj>.mcpcc-src`
//! sidecars at compile time and uses them at link time so the extractors can
//! still find the CLI definition (the pjsip/pjsua scenario).

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
fn link_recovers_sources_from_compile_step_sidecars() {
    let td = TempDir::new("mcpcc-sidecar");
    let cc_path = link_fake_compiler(&td.path);

    // CLI definition lives in a source whose object name gives no hint
    // (`output/obj2.o`), exactly like pjsip's link lines.
    std::fs::create_dir_all(td.path.join("src")).expect("mkdir src");
    std::fs::write(
        td.path.join("src/main.c"),
        b"int parse_args(int, char**);\nint main(int argc, char **argv) { return parse_args(argc, argv); }\n",
    )
    .expect("write main.c");
    std::fs::write(
        td.path.join("src/cli_config.c"),
        br#"
#include <getopt.h>

int parse_args(int argc, char **argv) {
  static struct option long_options[] = {
    {"verbose", no_argument, 0, 'v'},
    {"output", required_argument, 0, 'o'},
    {0, 0, 0, 0},
  };
  int opt;
  while ((opt = getopt_long(argc, argv, "vo:", long_options, 0)) != -1) {
    (void)opt;
  }
  return 0;
}
"#,
    )
    .expect("write cli_config.c");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let run = |args: &[&str]| {
        let out = Command::new(bin)
            .current_dir(&td.path)
            .env_remove("OPENROUTER_API_KEY")
            .env("MCPCC_ALLOW_NO_LLM", "1")
            .arg("--mcpcc-cc")
            .arg(&cc_path)
            .arg("--mcpcc-llm-mode")
            .arg("off")
            .arg("--")
            .args(args)
            .output()
            .expect("run mcpcc");
        assert!(
            out.status.success(),
            "mcpcc {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    // Compile steps: object names unrelated to the source names.
    run(&["-c", "src/main.c", "-o", "output/obj1.o"]);
    run(&["-c", "src/cli_config.c", "-o", "output/obj2.o"]);

    let sidecar = std::fs::read_to_string(td.path.join("output/obj2.o.mcpcc-src"))
        .expect("sidecar for obj2.o");
    assert!(
        sidecar.trim().ends_with("src/cli_config.c"),
        "sidecar must record the absolute source path, got: {sidecar}"
    );
    assert!(
        !td.path.join("output/obj1.o.mcp.json").exists(),
        "compile steps must not produce MCP artifacts"
    );

    // Link step: only opaque object names on the command line.
    run(&["output/obj1.o", "output/obj2.o", "-o", "bin/app"]);

    let contents = std::fs::read(td.path.join("bin/app.mcp.json")).expect("read mcp json");
    let v: serde_json::Value = serde_json::from_slice(&contents).expect("parse json");
    let tools = v
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");
    let structured = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some("app"))
        .expect("structured tool extracted via sidecar source recovery");

    let props = structured
        .get("inputSchema")
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object())
        .expect("properties");
    assert!(props.contains_key("verbose"), "missing verbose: {props:?}");
    assert!(props.contains_key("output"), "missing output: {props:?}");
}
