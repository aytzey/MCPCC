use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MCP_SPEC_VERSION: &str = "2025-11-25";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WrapperFlags {
    pub cc: Option<String>,
    pub print_cc: bool,
    pub artifacts_dir: Option<PathBuf>,
    pub mcp_json_out: Option<PathBuf>,
    pub server_out: Option<PathBuf>,
    pub manifest_out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    pub wrapper: WrapperFlags,
    pub passthrough: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliParseError {
    UnknownWrapperFlag(String),
    MissingValue(String),
}

impl std::fmt::Display for CliParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliParseError::UnknownWrapperFlag(flag) => {
                write!(f, "unknown mcpcc flag: {flag}")
            }
            CliParseError::MissingValue(flag) => write!(f, "missing value for {flag}"),
        }
    }
}

impl std::error::Error for CliParseError {}

/// Parse `mcpcc` argv (excluding argv[0]) into wrapper flags + compiler passthrough args.
pub fn parse_args(args: &[String]) -> Result<ParsedArgs, CliParseError> {
    let mut wrapper = WrapperFlags::default();
    let mut passthrough = Vec::new();

    if let Some(sep_idx) = args.iter().position(|a| a == "--") {
        let wrapper_args = &args[..sep_idx];
        let compiler_args = &args[(sep_idx + 1)..];
        parse_wrapper_args(wrapper_args, &mut wrapper)?;
        passthrough.extend(compiler_args.iter().cloned());
        return Ok(ParsedArgs {
            wrapper,
            passthrough,
        });
    }

    let mut idx = 0;
    while idx < args.len() {
        if consume_wrapper_flag(args, &mut idx, &mut wrapper)? {
            continue;
        }

        let arg = &args[idx];
        if arg.starts_with("--mcpcc-") {
            return Err(CliParseError::UnknownWrapperFlag(arg.clone()));
        }

        passthrough.push(arg.clone());
        idx += 1;
    }

    Ok(ParsedArgs {
        wrapper,
        passthrough,
    })
}

fn parse_wrapper_args(args: &[String], wrapper: &mut WrapperFlags) -> Result<(), CliParseError> {
    let mut idx = 0;
    while idx < args.len() {
        if consume_wrapper_flag(args, &mut idx, wrapper)? {
            continue;
        }

        return Err(CliParseError::UnknownWrapperFlag(args[idx].clone()));
    }

    Ok(())
}

fn consume_wrapper_flag(
    args: &[String],
    idx: &mut usize,
    wrapper: &mut WrapperFlags,
) -> Result<bool, CliParseError> {
    let arg = args
        .get(*idx)
        .ok_or_else(|| CliParseError::UnknownWrapperFlag("<missing arg>".to_string()))?;

    if arg == "--mcpcc-print-cc" {
        wrapper.print_cc = true;
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-cc=") {
        wrapper.cc = Some(value.to_string());
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-cc" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.cc = Some(value.clone());
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-artifacts-dir=") {
        wrapper.artifacts_dir = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-artifacts-dir" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.artifacts_dir = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-mcp-json-out=") {
        wrapper.mcp_json_out = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-mcp-json-out" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.mcp_json_out = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-server-out=") {
        wrapper.server_out = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-server-out" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.server_out = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-manifest-out=") {
        wrapper.manifest_out = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-manifest-out" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.manifest_out = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    Ok(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPaths {
    pub bin_path: PathBuf,
    pub base_name: String,
    pub mcp_json_path: PathBuf,
    pub server_path: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug)]
pub enum McpJsonWriteError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for McpJsonWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpJsonWriteError::Io(err) => write!(f, "{err}"),
            McpJsonWriteError::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for McpJsonWriteError {}

impl From<std::io::Error> for McpJsonWriteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for McpJsonWriteError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn build_minimal_mcp_json(artifacts: &ArtifactPaths) -> serde_json::Value {
    let tool_name = format!("{}.run_raw", artifacts.base_name);
    serde_json::json!({
        "mcpccVersion": env!("CARGO_PKG_VERSION"),
        "mcpSpecVersion": MCP_SPEC_VERSION,
        "binary": {
            "path": artifacts.bin_path.to_string_lossy(),
        },
        "tools": [
            {
                "name": tool_name,
                "description": "Run the target binary with raw argv and return stdout/stderr/exit code.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "argv": {
                            "type": "array",
                            "items": { "type": "string" },
                        },
                    },
                    "required": ["argv"],
                    "additionalProperties": false,
                },
            }
        ],
    })
}

pub fn write_mcp_json_atomic(artifacts: &ArtifactPaths) -> Result<(), McpJsonWriteError> {
    let mcp_json = build_minimal_mcp_json(artifacts);
    let bytes = serde_json::to_vec_pretty(&mcp_json)?;

    let out_path = &artifacts.mcp_json_path;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let parent_dir = out_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let pid = std::process::id();
    let tmp_path = parent_dir.join(format!(".mcpcc-tmp-{pid}-{unique}.json"));

    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }

    let read_back = std::fs::read(&tmp_path)?;
    let _: serde_json::Value = serde_json::from_slice(&read_back)?;

    if let Err(err) = std::fs::rename(&tmp_path, out_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(McpJsonWriteError::Io(err));
    }

    Ok(())
}

pub fn plan_artifacts(wrapper: &WrapperFlags, passthrough: &[String]) -> Option<ArtifactPaths> {
    if should_skip_mcp_artifact_generation(passthrough) {
        return None;
    }

    let bin_path = resolve_bin_path(passthrough);
    let base_name = bin_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "a.out".to_string());

    let default_dir = bin_path.parent().unwrap_or_else(|| Path::new("."));
    let dir = wrapper.artifacts_dir.as_deref().unwrap_or(default_dir);

    let mut mcp_json_path = dir.join(format!("{base_name}.mcp.json"));
    let mut server_path = dir.join(format!("{base_name}.mcp-server"));
    let mut manifest_path = dir.join(format!("{base_name}.mcpcc-manifest.json"));

    if let Some(path) = wrapper.mcp_json_out.as_ref() {
        mcp_json_path = path.clone();
    }
    if let Some(path) = wrapper.server_out.as_ref() {
        server_path = path.clone();
    }
    if let Some(path) = wrapper.manifest_out.as_ref() {
        manifest_path = path.clone();
    }

    Some(ArtifactPaths {
        bin_path,
        base_name,
        mcp_json_path,
        server_path,
        manifest_path,
    })
}

fn should_skip_mcp_artifact_generation(passthrough: &[String]) -> bool {
    passthrough
        .iter()
        .any(|arg| matches!(arg.as_str(), "-c" | "-E" | "-S" | "-shared" | "--shared"))
}

fn resolve_bin_path(passthrough: &[String]) -> PathBuf {
    if let Some(path) = parse_output_path(passthrough) {
        return PathBuf::from(path);
    }
    PathBuf::from("./a.out")
}

fn parse_output_path(passthrough: &[String]) -> Option<String> {
    let mut out: Option<String> = None;
    let mut idx = 0;
    while idx < passthrough.len() {
        let arg = &passthrough[idx];

        if arg == "-o" {
            idx += 1;
            if let Some(value) = passthrough.get(idx) {
                out = Some(value.clone());
            }
            idx += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("-o") {
            if !value.is_empty() {
                out = Some(value.to_string());
            }
        }

        idx += 1;
    }

    out
}

#[derive(Debug, Clone, Default)]
pub struct EnvSnapshot {
    pub mcpcc_cc: Option<String>,
    pub cc: Option<String>,
    pub path: Option<OsString>,
}

impl EnvSnapshot {
    pub fn from_current() -> Self {
        Self {
            mcpcc_cc: std::env::var("MCPCC_CC")
                .ok()
                .and_then(|v| (!v.trim().is_empty()).then_some(v)),
            cc: std::env::var("CC")
                .ok()
                .and_then(|v| (!v.trim().is_empty()).then_some(v)),
            path: std::env::var_os("PATH"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilerResolveError {
    NotFound {
        source: CompilerSource,
        spec: String,
    },
    NoCompilerFound,
}

impl std::fmt::Display for CompilerResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompilerResolveError::NotFound { source, spec } => {
                write!(f, "compiler not found for {source}: {spec}")
            }
            CompilerResolveError::NoCompilerFound => write!(
                f,
                "no compiler found (tried: --mcpcc-cc, MCPCC_CC, CC, clang, gcc)"
            ),
        }
    }
}

impl std::error::Error for CompilerResolveError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerSource {
    Flag,
    EnvMcpccCc,
    EnvCc,
}

impl std::fmt::Display for CompilerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompilerSource::Flag => write!(f, "--mcpcc-cc"),
            CompilerSource::EnvMcpccCc => write!(f, "MCPCC_CC"),
            CompilerSource::EnvCc => write!(f, "CC"),
        }
    }
}

pub fn resolve_underlying_compiler(
    wrapper: &WrapperFlags,
    env: &EnvSnapshot,
) -> Result<PathBuf, CompilerResolveError> {
    if let Some(spec) = wrapper.cc.as_deref() {
        return resolve_spec(spec, env.path.as_deref()).ok_or_else(|| {
            CompilerResolveError::NotFound {
                source: CompilerSource::Flag,
                spec: spec.to_string(),
            }
        });
    }

    if let Some(spec) = env.mcpcc_cc.as_deref() {
        return resolve_spec(spec, env.path.as_deref()).ok_or_else(|| {
            CompilerResolveError::NotFound {
                source: CompilerSource::EnvMcpccCc,
                spec: spec.to_string(),
            }
        });
    }

    if let Some(spec) = env.cc.as_deref() {
        return resolve_spec(spec, env.path.as_deref()).ok_or_else(|| {
            CompilerResolveError::NotFound {
                source: CompilerSource::EnvCc,
                spec: spec.to_string(),
            }
        });
    }

    if let Some(path) = resolve_spec("clang", env.path.as_deref()) {
        return Ok(path);
    }

    if let Some(path) = resolve_spec("gcc", env.path.as_deref()) {
        return Ok(path);
    }

    Err(CompilerResolveError::NoCompilerFound)
}

fn resolve_spec(spec: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    let program = if spec.contains('/') || spec.contains('\\') {
        spec
    } else {
        spec.split_whitespace().next().unwrap_or_default()
    };

    if program.is_empty() {
        return None;
    }

    find_executable(program, path_env)
}

fn find_executable(program: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') {
        let p = Path::new(program);
        return is_executable(p).then(|| p.to_path_buf());
    }

    let Some(path_env) = path_env else {
        return None;
    };

    for dir in std::env::split_paths(path_env) {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn write_exe(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write exe");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("chmod");
        }

        path
    }

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_args_with_separator_treats_pre_sep_as_wrapper() {
        let parsed = parse_args(&v(&[
            "--mcpcc-cc",
            "mycc",
            "--mcpcc-print-cc",
            "--",
            "-Wall",
            "hello.c",
        ]))
        .expect("parse");
        assert_eq!(parsed.wrapper.cc.as_deref(), Some("mycc"));
        assert!(parsed.wrapper.print_cc);
        assert_eq!(parsed.passthrough, v(&["-Wall", "hello.c"]));
    }

    #[test]
    fn parse_args_without_separator_only_consumes_mcpcc_prefixed_flags() {
        let parsed = parse_args(&v(&[
            "-Wall",
            "--mcpcc-cc=clang",
            "--mcpcc-artifacts-dir=artifacts",
            "hello.c",
            "--mcpcc-print-cc",
        ]))
        .expect("parse");
        assert_eq!(parsed.wrapper.cc.as_deref(), Some("clang"));
        assert!(parsed.wrapper.print_cc);
        assert_eq!(
            parsed.wrapper.artifacts_dir.as_deref(),
            Some(Path::new("artifacts"))
        );
        assert_eq!(parsed.passthrough, v(&["-Wall", "hello.c"]));
    }

    #[test]
    fn parse_args_errors_on_unknown_mcpcc_flag() {
        let err = parse_args(&v(&["--mcpcc-nope"])).expect_err("should fail");
        assert_eq!(
            err,
            CliParseError::UnknownWrapperFlag("--mcpcc-nope".to_string())
        );
    }

    #[test]
    fn resolve_compiler_prefers_flag_over_env_and_defaults() {
        let td = TempDir::new("mcpcc-resolve-flag");
        let flagcc = write_exe(&td.path, "flagcc");
        write_exe(&td.path, "clang");
        write_exe(&td.path, "gcc");

        let wrapper = WrapperFlags {
            cc: Some("flagcc".to_string()),
            ..WrapperFlags::default()
        };
        let env = EnvSnapshot {
            mcpcc_cc: Some("mcpccenv".to_string()),
            cc: Some("ccenv".to_string()),
            path: Some(td.path.as_os_str().to_os_string()),
        };

        let resolved = resolve_underlying_compiler(&wrapper, &env).expect("resolve");
        assert_eq!(resolved, flagcc);
    }

    #[test]
    fn resolve_compiler_prefers_mcpcc_cc_env_over_cc_env() {
        let td = TempDir::new("mcpcc-resolve-env");
        let mcpccenv = write_exe(&td.path, "mcpccenv");
        write_exe(&td.path, "ccenv");
        write_exe(&td.path, "clang");

        let wrapper = WrapperFlags::default();
        let env = EnvSnapshot {
            mcpcc_cc: Some("mcpccenv".to_string()),
            cc: Some("ccenv".to_string()),
            path: Some(td.path.as_os_str().to_os_string()),
        };

        let resolved = resolve_underlying_compiler(&wrapper, &env).expect("resolve");
        assert_eq!(resolved, mcpccenv);
    }

    #[test]
    fn resolve_compiler_prefers_clang_then_gcc_by_default() {
        let td = TempDir::new("mcpcc-resolve-defaults");
        let clang = write_exe(&td.path, "clang");
        write_exe(&td.path, "gcc");

        let wrapper = WrapperFlags::default();
        let env = EnvSnapshot {
            path: Some(td.path.as_os_str().to_os_string()),
            ..EnvSnapshot::default()
        };

        let resolved = resolve_underlying_compiler(&wrapper, &env).expect("resolve");
        assert_eq!(resolved, clang);
    }

    #[test]
    fn artifact_plan_skips_compile_only_flags() {
        for flag in ["-c", "-E", "-S", "-shared"] {
            let wrapper = WrapperFlags::default();
            let passthrough = v(&["hello.c", flag, "-o", "hello"]);
            assert_eq!(plan_artifacts(&wrapper, &passthrough), None, "flag: {flag}");
        }
    }

    #[test]
    fn artifact_plan_defaults_to_a_out_when_linking() {
        let wrapper = WrapperFlags::default();
        let plan = plan_artifacts(&wrapper, &v(&["hello.c"])).expect("plan");
        assert_eq!(plan.bin_path, PathBuf::from("./a.out"));
        assert_eq!(plan.base_name, "a.out");
        assert_eq!(plan.mcp_json_path, PathBuf::from("./a.out.mcp.json"));
        assert_eq!(plan.server_path, PathBuf::from("./a.out.mcp-server"));
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("./a.out.mcpcc-manifest.json")
        );
    }

    #[test]
    fn artifact_plan_uses_o_flag_for_bin_path_and_default_artifact_naming() {
        let wrapper = WrapperFlags::default();
        let plan = plan_artifacts(&wrapper, &v(&["hello.c", "-o", "bin/hello"])).expect("plan");
        assert_eq!(plan.bin_path, PathBuf::from("bin/hello"));
        assert_eq!(plan.base_name, "hello");
        assert_eq!(plan.mcp_json_path, PathBuf::from("bin/hello.mcp.json"));
        assert_eq!(plan.server_path, PathBuf::from("bin/hello.mcp-server"));
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("bin/hello.mcpcc-manifest.json")
        );
    }

    #[test]
    fn artifact_plan_respects_artifacts_dir_override() {
        let wrapper = WrapperFlags {
            artifacts_dir: Some(PathBuf::from("artifacts")),
            ..WrapperFlags::default()
        };
        let plan = plan_artifacts(&wrapper, &v(&["hello.c", "-o", "bin/hello"])).expect("plan");
        assert_eq!(plan.bin_path, PathBuf::from("bin/hello"));
        assert_eq!(
            plan.mcp_json_path,
            PathBuf::from("artifacts/hello.mcp.json")
        );
        assert_eq!(
            plan.server_path,
            PathBuf::from("artifacts/hello.mcp-server")
        );
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("artifacts/hello.mcpcc-manifest.json")
        );
    }

    #[test]
    fn artifact_plan_respects_individual_output_overrides() {
        let wrapper = WrapperFlags {
            artifacts_dir: Some(PathBuf::from("artifacts")),
            mcp_json_out: Some(PathBuf::from("custom/tool.mcp.json")),
            server_out: Some(PathBuf::from("custom/tool.mcp-server")),
            manifest_out: Some(PathBuf::from("custom/tool.mcpcc-manifest.json")),
            ..WrapperFlags::default()
        };
        let plan = plan_artifacts(&wrapper, &v(&["hello.c", "-o", "bin/hello"])).expect("plan");
        assert_eq!(plan.bin_path, PathBuf::from("bin/hello"));
        assert_eq!(plan.mcp_json_path, PathBuf::from("custom/tool.mcp.json"));
        assert_eq!(plan.server_path, PathBuf::from("custom/tool.mcp-server"));
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("custom/tool.mcpcc-manifest.json")
        );
    }
}
