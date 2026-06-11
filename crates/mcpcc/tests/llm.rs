use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
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

struct CapturedRequest {
    request_line: String,
    headers: String,
    body: String,
}

fn start_openrouter_mock(
    status: u16,
    body: String,
) -> Option<(String, mpsc::Receiver<CapturedRequest>)> {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(v) => v,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping: sandbox disallows binding TCP listeners");
            return None;
        }
        Err(err) => panic!("bind: {err}"),
    };
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(&mut stream);

        let mut request_line = String::new();
        reader
            .read_line(&mut request_line)
            .expect("read request line");

        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read header line");
            if line == "\r\n" || line.is_empty() {
                break;
            }
            headers.push_str(&line);
        }

        let mut content_length: usize = 0;
        for line in headers.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("content-length:") {
                content_length = rest.trim().parse().unwrap_or(0);
            }
        }

        let mut body_bytes = vec![0u8; content_length];
        reader.read_exact(&mut body_bytes).expect("read body");
        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        tx.send(CapturedRequest {
            request_line,
            headers,
            body: body_str,
        })
        .expect("send captured request");

        let status_text = match status {
            200 => "OK",
            500 => "Internal Server Error",
            _ => "OK",
        };
        let body_bytes = body.as_bytes();
        let response = format!(
            "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body_bytes.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        stream.write_all(body_bytes).expect("write response body");
    });

    Some((format!("http://{addr}"), rx))
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();

    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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
fn required_mode_calls_openrouter_and_writes_cache_then_uses_cache_without_key() {
    let td = TempDir::new("mcpcc-llm-required");
    let cache_dir = td.path.join("cache");
    let cc_path = link_fake_compiler(&td.path);

    let model = "test-model";
    let content_json = serde_json::json!({
        "tools": {
            "hello.run_raw": {
                "toolDescription": "Run the program with raw argv.",
                "params": {
                    "argv": "Program arguments (argv array).",
                    "stdin": "Text piped to the program's standard input."
                }
            }
        }
    })
    .to_string();
    let response_body = serde_json::json!({
        "choices": [
            { "message": { "content": content_json } }
        ]
    })
    .to_string();

    let Some((base_url, rx)) = start_openrouter_mock(200, response_body) else {
        return;
    };

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env("OPENROUTER_API_KEY", "testkey")
        .env("MCPCC_OPENROUTER_BASE_URL", &base_url)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--mcpcc-llm-model")
        .arg(model)
        .arg("--mcpcc-llm-mode")
        .arg("required")
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

    let captured = rx.recv().expect("captured request");
    assert!(
        captured.request_line.contains("POST /chat/completions"),
        "unexpected request line: {}",
        captured.request_line
    );
    assert!(
        captured.headers.contains("Authorization: Bearer testkey"),
        "missing Authorization header: {}",
        captured.headers
    );

    let body_json: serde_json::Value =
        serde_json::from_str(&captured.body).expect("parse openrouter request body");
    assert_eq!(
        body_json.get("temperature").and_then(|v| v.as_i64()),
        Some(0)
    );

    let analysis_summary_json = serde_json::json!({
        "binaryName": "hello",
        "tools": [{
            "toolName": "hello.run_raw",
            "binaryName": "hello",
            "params": [{
                "property": "argv",
                "long": null,
                "short": null,
                "takesValue": true,
                "optionalArg": true,
                "guessedType": "array<string>",
                "doc": "Arguments to pass to the binary as an argv array."
            }, {
                "property": "stdin",
                "long": null,
                "short": null,
                "takesValue": true,
                "optionalArg": true,
                "guessedType": "string",
                "doc": "Optional text piped to the program's standard input."
            }]
        }]
    })
    .to_string();
    let cache_key = sha256_hex(&format!(
        "{}{}{}",
        mcpcc::LLM_PROMPT_VERSION,
        model,
        analysis_summary_json
    ));
    let cache_path = cache_dir.join("llm").join(format!("{cache_key}.json"));
    assert!(cache_path.exists(), "expected cache file to exist");

    let mcp_json_path = td.path.join("bin/hello.mcp.json");
    let mcp_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&mcp_json_path).expect("read mcp json"))
            .expect("parse mcp json");
    let tool = mcp_json
        .get("tools")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .expect("tool entry");
    assert_eq!(
        tool.get("description").and_then(|v| v.as_str()),
        Some("Run the program with raw argv.")
    );
    assert_eq!(
        tool.get("inputSchema")
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.get("argv"))
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str()),
        Some("Program arguments (argv array).")
    );

    let manifest_path = td.path.join("bin/hello.mcpcc-manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let llm = manifest
        .get("llm")
        .and_then(|v| v.as_object())
        .expect("llm");
    assert_eq!(llm.get("mode").and_then(|v| v.as_str()), Some("required"));
    assert_eq!(llm.get("cacheHit").and_then(|v| v.as_bool()), Some(false));

    let out = Command::new(bin)
        .current_dir(&td.path)
        .env_remove("OPENROUTER_API_KEY")
        .env("MCPCC_OPENROUTER_BASE_URL", "http://127.0.0.1:9")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--mcpcc-llm-model")
        .arg(model)
        .arg("--mcpcc-llm-mode")
        .arg("required")
        .arg("--")
        .arg("hello.c")
        .arg("-o")
        .arg("bin/hello")
        .output()
        .expect("run mcpcc (cache hit)");

    assert!(
        out.status.success(),
        "expected exit 0 from cache hit, got: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let llm = manifest
        .get("llm")
        .and_then(|v| v.as_object())
        .expect("llm");
    assert_eq!(llm.get("cacheHit").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn best_effort_without_key_uses_placeholder_and_records_manifest() {
    let td = TempDir::new("mcpcc-llm-best-effort");
    let cache_dir = td.path.join("cache");
    let cc_path = link_fake_compiler(&td.path);

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

    let manifest_path = td.path.join("bin/hello.mcpcc-manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let llm = manifest
        .get("llm")
        .and_then(|v| v.as_object())
        .expect("llm");
    assert_eq!(
        llm.get("usedPlaceholder").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(
        llm.get("error").and_then(|v| v.as_str()).is_some(),
        "expected placeholder reason in llm.error"
    );
}

#[test]
fn llm_mode_off_requires_allow_env() {
    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .env_remove("MCPCC_ALLOW_NO_LLM")
        .arg("--mcpcc-llm-mode")
        .arg("off")
        .output()
        .expect("run mcpcc");
    assert_eq!(out.status.code(), Some(2));

    let td = TempDir::new("mcpcc-llm-off-allowed");
    let cc_path = write_exe(&td.path, "mycc", b"#!/bin/sh\nexit 0\n");
    let out = Command::new(bin)
        .env("MCPCC_ALLOW_NO_LLM", "1")
        .arg("--mcpcc-llm-mode")
        .arg("off")
        .arg("--mcpcc-print-cc")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .output()
        .expect("run mcpcc");
    assert!(out.status.success());
}

#[test]
fn required_mode_openrouter_failure_exits_70() {
    let td = TempDir::new("mcpcc-llm-required-failure");
    let cache_dir = td.path.join("cache");
    let cc_path = link_fake_compiler(&td.path);

    let Some((base_url, _rx)) = start_openrouter_mock(500, "{}".to_string()) else {
        return;
    };

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env("OPENROUTER_API_KEY", "testkey")
        .env("MCPCC_OPENROUTER_BASE_URL", &base_url)
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--mcpcc-llm-model")
        .arg("test-model")
        .arg("--mcpcc-llm-mode")
        .arg("required")
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
        "failed LLM should not write mcp.json"
    );
    assert!(
        !td.path.join("bin/hello.mcpcc-manifest.json").exists(),
        "failed LLM should not write manifest"
    );
}

#[test]
fn required_mode_real_openrouter_smoke() {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .ok()
        .and_then(|v| (!v.trim().is_empty()).then_some(v));
    if api_key.is_none() {
        eprintln!("skipping: OPENROUTER_API_KEY not set");
        return;
    }
    if ("openrouter.ai", 443).to_socket_addrs().is_err() {
        eprintln!("skipping: cannot resolve openrouter.ai (DNS/network unavailable)");
        return;
    }

    let td = TempDir::new("mcpcc-llm-required-real");
    let cache_dir = td.path.join("cache");
    let cc_path = link_fake_compiler(&td.path);

    let bin = env!("CARGO_BIN_EXE_mcpcc");
    let out = Command::new(bin)
        .current_dir(&td.path)
        .env("OPENROUTER_API_KEY", api_key.unwrap())
        .env_remove("MCPCC_OPENROUTER_BASE_URL")
        .arg("--mcpcc-cc")
        .arg(&cc_path)
        .arg("--mcpcc-cache-dir")
        .arg(&cache_dir)
        .arg("--mcpcc-llm-mode")
        .arg("required")
        .arg("--")
        .arg("hello.c")
        .arg("-o")
        .arg("bin/hello")
        .output()
        .expect("run mcpcc");

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("Dns Failed") || stderr.contains("failed to lookup address") {
            eprintln!("skipping: OpenRouter unreachable (DNS/network unavailable)");
            return;
        }
        panic!(
            "expected exit 0, got: {:?}\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            stderr
        );
    }

    let manifest_path = td.path.join("bin/hello.mcpcc-manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let llm = manifest
        .get("llm")
        .and_then(|v| v.as_object())
        .expect("llm");
    assert_eq!(llm.get("mode").and_then(|v| v.as_str()), Some("required"));
    assert_eq!(
        llm.get("usedPlaceholder").and_then(|v| v.as_bool()),
        Some(false),
        "expected a real OpenRouter call (no placeholder)"
    );

    let llm_dir = cache_dir.join("llm");
    let cache_files: Vec<_> = std::fs::read_dir(&llm_dir)
        .expect("read cache llm dir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!cache_files.is_empty(), "expected LLM cache files");
}
