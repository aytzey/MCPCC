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
fn passthrough_invokes_compiler_with_identical_args_in_order() {
    let td = TempDir::new("mcpcc-passthrough-args");
    let argv_out = td.path.join("argv.txt");
    let cc_path = write_exe(
        &td.path,
        "fakecc",
        br#"#!/bin/sh
set -eu
out="${MCPCC_TEST_ARGV_OUT:-}"
if [ -z "$out" ]; then
  echo "MCPCC_TEST_ARGV_OUT not set" >&2
  exit 99
fi
printf '%s\n' "$@" > "$out"
exit 0
"#,
    );

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--")
        .arg("-Wall")
        .arg("hello.c")
        .arg("-o")
        .arg("hello")
        .env("MCPCC_TEST_ARGV_OUT", &argv_out)
        .output()
        .expect("run mcpcc");

    assert!(
        out.status.success(),
        "expected exit 0, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let argv_contents = std::fs::read_to_string(&argv_out).expect("read argv output");
    let got: Vec<&str> = argv_contents.lines().collect();
    assert_eq!(got, vec!["-Wall", "hello.c", "-o", "hello"]);
}

#[test]
fn nonzero_compiler_exit_code_is_propagated() {
    let td = TempDir::new("mcpcc-passthrough-exit");
    let cc_path = write_exe(&td.path, "fakecc", b"#!/bin/sh\nexit 7\n");

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--")
        .arg("hello.c")
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
}

#[test]
fn wrapper_usage_errors_exit_2() {
    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .arg("--mcpcc-nope")
        .output()
        .expect("run mcpcc");

    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
