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

fn packaged_server_binary() -> PathBuf {
    let mcpcc_bin = PathBuf::from(env!("CARGO_BIN_EXE_mcpcc"));
    let dir = mcpcc_bin.parent().expect("mcpcc bin parent");

    let name = if cfg!(windows) {
        "mcpcc-mcp-server.exe"
    } else {
        "mcpcc-mcp-server"
    };
    dir.join(name)
}

fn read_prefix(path: &Path, len: usize) -> Vec<u8> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).expect("open");
    let mut buf = vec![0u8; len];
    let n = f.read(&mut buf).expect("read");
    buf.truncate(n);
    buf
}

fn fake_linking_cc(dir: &Path) -> PathBuf {
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
    )
}

#[test]
fn copies_mcp_server_binary_on_successful_link() {
    let td = TempDir::new("mcpcc-server-copy-link");
    let cache_dir = td.path.join("cache");
    let cc_path = fake_linking_cc(&td.path);

    let server_src = packaged_server_binary();
    assert!(
        server_src.exists(),
        "expected packaged server binary at {}",
        server_src.display()
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

    let server_out = td.path.join("bin/hello.mcp-server");
    assert!(server_out.is_file(), "server binary must be copied");

    let src_meta = std::fs::metadata(&server_src).expect("server src metadata");
    let out_meta = std::fs::metadata(&server_out).expect("server out metadata");
    assert_eq!(src_meta.len(), out_meta.len(), "server copy size mismatch");
    assert_eq!(
        read_prefix(&server_src, 64),
        read_prefix(&server_out, 64),
        "server copy prefix mismatch"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(
            out_meta.permissions().mode() & 0o111 != 0,
            "copied server binary must be executable"
        );
    }
}

#[test]
fn respects_mcpcc_server_out_override_and_records_in_manifest() {
    let td = TempDir::new("mcpcc-server-copy-override");
    let cache_dir = td.path.join("cache");
    let cc_path = fake_linking_cc(&td.path);

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
        .arg("--mcpcc-server-out")
        .arg("custom/tool.mcp-server")
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

    assert!(
        td.path.join("custom/tool.mcp-server").is_file(),
        "server binary must be copied to override path"
    );

    let manifest_path = td.path.join("bin/hello.mcpcc-manifest.json");
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest json");
    assert_eq!(
        v.get("artifacts")
            .and_then(|a| a.get("server"))
            .and_then(|v| v.as_str()),
        Some("custom/tool.mcp-server")
    );
}

#[test]
fn does_not_copy_server_for_compile_only_invocations() {
    let td = TempDir::new("mcpcc-server-copy-compile-only");
    let cc_path = fake_linking_cc(&td.path);

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
        !td.path.join("obj/hello.o.mcp-server").exists(),
        "compile-only mode must not produce per-binary server"
    );
}

#[test]
fn does_not_copy_server_when_compiler_fails() {
    let td = TempDir::new("mcpcc-server-copy-compiler-fails");
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
        !td.path.join("bin/hello.mcp-server").exists(),
        "non-zero compiler exit must not produce per-binary server"
    );
}

#[test]
fn server_copy_failure_exits_70_and_skips_manifest() {
    let td = TempDir::new("mcpcc-server-copy-fails");
    let cache_dir = td.path.join("cache");
    let cc_path = fake_linking_cc(&td.path);
    std::fs::create_dir_all(td.path.join("serverdir")).expect("create serverdir");

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
        .arg("--mcpcc-server-out")
        .arg("serverdir")
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
        !td.path.join("bin/hello.mcpcc-manifest.json").exists(),
        "server copy failure must not produce manifest"
    );
}
