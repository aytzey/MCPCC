use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WrapperFlags {
    pub cc: Option<String>,
    pub print_cc: bool,
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
        let arg = &args[idx];

        if arg == "--mcpcc-print-cc" {
            wrapper.print_cc = true;
            idx += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--mcpcc-cc=") {
            wrapper.cc = Some(value.to_string());
            idx += 1;
            continue;
        }

        if arg == "--mcpcc-cc" {
            idx += 1;
            let Some(value) = args.get(idx) else {
                return Err(CliParseError::MissingValue(arg.clone()));
            };
            wrapper.cc = Some(value.clone());
            idx += 1;
            continue;
        }

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
        let arg = &args[idx];

        if arg == "--mcpcc-print-cc" {
            wrapper.print_cc = true;
            idx += 1;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--mcpcc-cc=") {
            wrapper.cc = Some(value.to_string());
            idx += 1;
            continue;
        }

        if arg == "--mcpcc-cc" {
            idx += 1;
            let Some(value) = args.get(idx) else {
                return Err(CliParseError::MissingValue(arg.clone()));
            };
            wrapper.cc = Some(value.clone());
            idx += 1;
            continue;
        }

        return Err(CliParseError::UnknownWrapperFlag(arg.clone()));
    }

    Ok(())
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
            "hello.c",
            "--mcpcc-print-cc",
        ]))
        .expect("parse");
        assert_eq!(parsed.wrapper.cc.as_deref(), Some("clang"));
        assert!(parsed.wrapper.print_cc);
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
            print_cc: false,
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
}
