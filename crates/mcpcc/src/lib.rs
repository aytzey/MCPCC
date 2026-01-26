use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub const MCP_SPEC_VERSION: &str = "2025-11-25";
pub const LLM_PROMPT_VERSION: &str = "v1";
pub const DEFAULT_LLM_MODEL: &str = "openai/gpt-4o-mini";
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmMode {
    Required,
    BestEffort,
    Off,
}

impl Default for LlmMode {
    fn default() -> Self {
        Self::Required
    }
}

impl LlmMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LlmMode::Required => "required",
            LlmMode::BestEffort => "best-effort",
            LlmMode::Off => "off",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "required" => Some(LlmMode::Required),
            "best-effort" => Some(LlmMode::BestEffort),
            "off" => Some(LlmMode::Off),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WrapperFlags {
    pub cc: Option<String>,
    pub print_cc: bool,
    pub verbose: bool,
    pub artifacts_dir: Option<PathBuf>,
    pub mcp_json_out: Option<PathBuf>,
    pub server_out: Option<PathBuf>,
    pub manifest_out: Option<PathBuf>,
    pub llm_mode: LlmMode,
    pub llm_model: Option<String>,
    pub cache_dir: Option<PathBuf>,
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
    InvalidValue { flag: String, value: String },
}

impl std::fmt::Display for CliParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliParseError::UnknownWrapperFlag(flag) => {
                write!(f, "unknown mcpcc flag: {flag}")
            }
            CliParseError::MissingValue(flag) => write!(f, "missing value for {flag}"),
            CliParseError::InvalidValue { flag, value } => {
                write!(f, "invalid value for {flag}: {value}")
            }
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

    if arg == "--mcpcc-verbose" {
        wrapper.verbose = true;
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

    if let Some(value) = arg.strip_prefix("--mcpcc-llm-mode=") {
        let Some(mode) = LlmMode::parse(value) else {
            return Err(CliParseError::InvalidValue {
                flag: "--mcpcc-llm-mode".to_string(),
                value: value.to_string(),
            });
        };
        wrapper.llm_mode = mode;
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-llm-mode" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        let Some(mode) = LlmMode::parse(value) else {
            return Err(CliParseError::InvalidValue {
                flag: "--mcpcc-llm-mode".to_string(),
                value: value.clone(),
            });
        };
        wrapper.llm_mode = mode;
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-llm-model=") {
        wrapper.llm_model = Some(value.to_string());
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-llm-model" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.llm_model = Some(value.clone());
        *idx += 1;
        return Ok(true);
    }

    if let Some(value) = arg.strip_prefix("--mcpcc-cache-dir=") {
        wrapper.cache_dir = Some(PathBuf::from(value));
        *idx += 1;
        return Ok(true);
    }

    if arg == "--mcpcc-cache-dir" {
        *idx += 1;
        let Some(value) = args.get(*idx) else {
            return Err(CliParseError::MissingValue(arg.clone()));
        };
        wrapper.cache_dir = Some(PathBuf::from(value));
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
pub enum JsonWriteError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

pub type McpJsonWriteError = JsonWriteError;

impl std::fmt::Display for JsonWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonWriteError::Io(err) => write!(f, "{err}"),
            JsonWriteError::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for JsonWriteError {}

impl From<std::io::Error> for JsonWriteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for JsonWriteError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmToolDescriptions {
    pub tool_description: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmManifestInfo {
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub cache_hit: bool,
    pub prompt_version: String,
    pub used_placeholder: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LlmEnv {
    pub openrouter_api_key: Option<String>,
    pub allow_no_llm: bool,
    pub openrouter_base_url: Option<String>,
}

impl LlmEnv {
    pub fn from_current() -> Self {
        let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
            .ok()
            .and_then(|v| (!v.trim().is_empty()).then_some(v));
        let allow_no_llm = matches!(std::env::var("MCPCC_ALLOW_NO_LLM"), Ok(v) if v.trim() == "1");
        let openrouter_base_url = std::env::var("MCPCC_OPENROUTER_BASE_URL")
            .ok()
            .and_then(|v| (!v.trim().is_empty()).then_some(v));
        Self {
            openrouter_api_key,
            allow_no_llm,
            openrouter_base_url,
        }
    }
}

#[derive(Debug)]
pub enum LlmError {
    MissingApiKey,
    Io(std::io::Error),
    Json(serde_json::Error),
    Http(String),
    InvalidOutput(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::MissingApiKey => write!(f, "OPENROUTER_API_KEY is not set"),
            LlmError::Io(err) => write!(f, "{err}"),
            LlmError::Json(err) => write!(f, "{err}"),
            LlmError::Http(msg) => write!(f, "{msg}"),
            LlmError::InvalidOutput(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for LlmError {}

impl From<std::io::Error> for LlmError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for LlmError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<JsonWriteError> for LlmError {
    fn from(value: JsonWriteError) -> Self {
        match value {
            JsonWriteError::Io(err) => Self::Io(err),
            JsonWriteError::Json(err) => Self::Json(err),
        }
    }
}

pub fn build_minimal_mcp_json(
    artifacts: &ArtifactPaths,
    descriptions: &LlmToolDescriptions,
) -> serde_json::Value {
    let tool_name = format!("{}.run_raw", artifacts.base_name);
    let argv_description = descriptions
        .params
        .get("argv")
        .map(String::as_str)
        .unwrap_or("Arguments to pass to the binary as an argv array.");
    serde_json::json!({
        "mcpccVersion": env!("CARGO_PKG_VERSION"),
        "mcpSpecVersion": MCP_SPEC_VERSION,
        "binary": {
            "path": artifacts.bin_path.to_string_lossy(),
        },
        "tools": [
            {
                "name": tool_name,
                "description": descriptions.tool_description,
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "argv": {
                            "type": "array",
                            "description": argv_description,
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

pub fn write_mcp_json_atomic(
    artifacts: &ArtifactPaths,
    descriptions: &LlmToolDescriptions,
) -> Result<(), JsonWriteError> {
    let mcp_json = build_minimal_mcp_json(artifacts, descriptions);
    write_json_atomic(&artifacts.mcp_json_path, &mcp_json)?;

    Ok(())
}

pub fn build_manifest_json(
    compiler: &Path,
    compiler_args: &[String],
    compiler_exit_code: i32,
    artifacts: &ArtifactPaths,
    llm: &LlmManifestInfo,
) -> serde_json::Value {
    let compiler_path = compiler.to_string_lossy();
    let mut argv = Vec::with_capacity(1 + compiler_args.len());
    argv.push(compiler_path.to_string());
    argv.extend(compiler_args.iter().cloned());

    serde_json::json!({
        "mcpccVersion": env!("CARGO_PKG_VERSION"),
        "mcpSpecVersion": MCP_SPEC_VERSION,
        "binary": {
            "path": artifacts.bin_path.to_string_lossy(),
        },
        "compiler": {
            "cc": compiler_path,
            "argv": argv,
            "exitCode": compiler_exit_code,
        },
        "analysis": {
            "usedLibclang": false,
            "extractors": [],
            "structuredToolGenerated": false,
            "paramCount": 0,
            "notes": [],
        },
        "llm": {
            "mode": llm.mode,
            "provider": llm.provider,
            "model": llm.model,
            "cacheHit": llm.cache_hit,
            "promptVersion": llm.prompt_version,
            "usedPlaceholder": llm.used_placeholder,
            "error": llm.error,
        },
        "artifacts": {
            "mcpJson": artifacts.mcp_json_path.to_string_lossy(),
            "server": artifacts.server_path.to_string_lossy(),
            "manifest": artifacts.manifest_path.to_string_lossy(),
        },
    })
}

pub fn write_manifest_json_atomic(
    compiler: &Path,
    compiler_args: &[String],
    compiler_exit_code: i32,
    artifacts: &ArtifactPaths,
    llm: &LlmManifestInfo,
) -> Result<(), JsonWriteError> {
    let manifest = build_manifest_json(compiler, compiler_args, compiler_exit_code, artifacts, llm);
    write_json_atomic(&artifacts.manifest_path, &manifest)?;
    Ok(())
}

pub fn generate_run_raw_llm_descriptions(
    artifacts: &ArtifactPaths,
    wrapper: &WrapperFlags,
    llm_env: &LlmEnv,
) -> Result<(LlmToolDescriptions, LlmManifestInfo), LlmError> {
    let provider = "openrouter".to_string();
    let mode = wrapper.llm_mode;
    let model = resolve_llm_model(wrapper);
    let prompt_version = LLM_PROMPT_VERSION.to_string();

    if mode == LlmMode::Off {
        return Ok((
            placeholder_run_raw_descriptions(),
            LlmManifestInfo {
                mode: mode.as_str().to_string(),
                provider,
                model,
                cache_hit: false,
                prompt_version,
                used_placeholder: true,
                error: Some("llm mode off".to_string()),
            },
        ));
    }

    let analysis_summary_json = run_raw_analysis_summary_json(&artifacts.base_name);
    let cache_dir = resolve_cache_dir(wrapper);
    let cache_key = llm_cache_key_hex(LLM_PROMPT_VERSION, &model, analysis_summary_json.as_str());
    let cache_path = cache_dir.join("llm").join(format!("{cache_key}.json"));

    if let Ok(descriptions) = read_llm_cache(&cache_path) {
        return Ok((
            descriptions,
            LlmManifestInfo {
                mode: mode.as_str().to_string(),
                provider,
                model,
                cache_hit: true,
                prompt_version,
                used_placeholder: false,
                error: None,
            },
        ));
    }

    let api_key = match llm_env
        .openrouter_api_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => v,
        None => {
            if mode == LlmMode::Required {
                return Err(LlmError::MissingApiKey);
            }
            return Ok((
                placeholder_run_raw_descriptions(),
                LlmManifestInfo {
                    mode: mode.as_str().to_string(),
                    provider,
                    model,
                    cache_hit: false,
                    prompt_version,
                    used_placeholder: true,
                    error: Some("OPENROUTER_API_KEY missing".to_string()),
                },
            ));
        }
    };

    let base_url = llm_env
        .openrouter_base_url
        .as_deref()
        .unwrap_or(DEFAULT_OPENROUTER_BASE_URL);

    match call_openrouter(api_key, base_url, &model, analysis_summary_json.as_str()) {
        Ok(descriptions) => {
            let cache_value = serde_json::json!({
                "toolDescription": descriptions.tool_description.clone(),
                "params": descriptions.params.clone(),
            });
            write_json_atomic(&cache_path, &cache_value)?;
            Ok((
                descriptions,
                LlmManifestInfo {
                    mode: mode.as_str().to_string(),
                    provider,
                    model,
                    cache_hit: false,
                    prompt_version,
                    used_placeholder: false,
                    error: None,
                },
            ))
        }
        Err(err) => {
            if mode == LlmMode::Required {
                return Err(err);
            }
            Ok((
                placeholder_run_raw_descriptions(),
                LlmManifestInfo {
                    mode: mode.as_str().to_string(),
                    provider,
                    model,
                    cache_hit: false,
                    prompt_version,
                    used_placeholder: true,
                    error: Some("OpenRouter request failed".to_string()),
                },
            ))
        }
    }
}

fn write_json_atomic(out_path: &Path, json: &serde_json::Value) -> Result<(), JsonWriteError> {
    let bytes = serde_json::to_vec_pretty(json)?;

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
        return Err(JsonWriteError::Io(err));
    }

    Ok(())
}

fn resolve_llm_model(wrapper: &WrapperFlags) -> String {
    wrapper
        .llm_model
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_LLM_MODEL)
        .to_string()
}

fn resolve_cache_dir(wrapper: &WrapperFlags) -> PathBuf {
    wrapper
        .cache_dir
        .as_ref()
        .filter(|p| !p.as_os_str().is_empty())
        .cloned()
        .unwrap_or_else(default_cache_dir)
}

fn default_cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("mcpcc");
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".cache").join("mcpcc");
        }
    }

    PathBuf::from(".mcpcc-cache")
}

fn run_raw_analysis_summary_json(base_name: &str) -> String {
    serde_json::json!({
        "toolName": format!("{base_name}.run_raw"),
        "params": ["argv"],
    })
    .to_string()
}

fn llm_cache_key_hex(prompt_version: &str, model: &str, analysis_summary_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt_version.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(analysis_summary_json.as_bytes());
    let digest = hasher.finalize();

    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn placeholder_run_raw_descriptions() -> LlmToolDescriptions {
    let mut params = BTreeMap::new();
    params.insert(
        "argv".to_string(),
        "Arguments to pass to the binary as an argv array.".to_string(),
    );
    LlmToolDescriptions {
        tool_description: "Run the target binary with raw argv and return stdout/stderr/exit code."
            .to_string(),
        params,
    }
}

fn sanitize_description(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let mut out = String::new();
    let mut count = 0usize;
    for ch in trimmed.chars() {
        if count >= 240 {
            break;
        }
        out.push(ch);
        count += 1;
    }

    (count >= 5).then_some(out)
}

fn parse_run_raw_llm_output(value: &serde_json::Value) -> Result<LlmToolDescriptions, LlmError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LlmError::InvalidOutput("LLM output must be a JSON object".to_string()))?;

    let tool_description_raw = obj
        .get("toolDescription")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            LlmError::InvalidOutput("LLM output missing toolDescription string".to_string())
        })?;
    let Some(tool_description) = sanitize_description(tool_description_raw) else {
        return Err(LlmError::InvalidOutput(
            "toolDescription must be 5–240 characters".to_string(),
        ));
    };

    let params_obj = obj
        .get("params")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LlmError::InvalidOutput("LLM output missing params object".to_string()))?;
    let argv_raw = params_obj
        .get("argv")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            LlmError::InvalidOutput("LLM output missing params.argv string".to_string())
        })?;
    let Some(argv_description) = sanitize_description(argv_raw) else {
        return Err(LlmError::InvalidOutput(
            "params.argv must be 5–240 characters".to_string(),
        ));
    };

    let mut params = BTreeMap::new();
    params.insert("argv".to_string(), argv_description);

    Ok(LlmToolDescriptions {
        tool_description,
        params,
    })
}

fn parse_llm_output_str(content: &str) -> Result<LlmToolDescriptions, LlmError> {
    let content = content.trim();

    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => {
            let Some(start) = content.find('{') else {
                return Err(LlmError::InvalidOutput(
                    "LLM output was not valid JSON".to_string(),
                ));
            };
            let Some(end) = content.rfind('}') else {
                return Err(LlmError::InvalidOutput(
                    "LLM output was not valid JSON".to_string(),
                ));
            };
            serde_json::from_str(&content[start..=end])
                .map_err(|_| LlmError::InvalidOutput("LLM output was not valid JSON".to_string()))?
        }
    };

    parse_run_raw_llm_output(&parsed)
}

fn read_llm_cache(path: &Path) -> Result<LlmToolDescriptions, LlmError> {
    let bytes = std::fs::read(path)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    parse_run_raw_llm_output(&value)
}

fn call_openrouter(
    api_key: &str,
    base_url: &str,
    model: &str,
    analysis_summary_json: &str,
) -> Result<LlmToolDescriptions, LlmError> {
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{base_url}/chat/completions");

    let system_prompt = concat!(
        "You generate short plain-text descriptions for an MCP tool. ",
        "Return ONLY a JSON object with keys: toolDescription (string) and params (object). ",
        "Do not use markdown or code fences. ",
        "All descriptions must be 5–240 characters after trimming.",
    );
    let user_prompt = format!(
        "analysis_summary_json:\n{analysis_summary_json}\n\nReturn JSON: \
{{\"toolDescription\":\"...\",\"params\":{{\"argv\":\"...\"}}}}"
    );

    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt },
        ],
        "response_format": { "type": "json_object" },
    });

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(20))
        .build();

    let response = agent
        .post(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .send_json(body);

    let response = match response {
        Ok(v) => v,
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            return Err(LlmError::Http(format!(
                "OpenRouter request failed with HTTP {code}: {body}"
            )));
        }
        Err(ureq::Error::Transport(err)) => {
            return Err(LlmError::Http(format!("OpenRouter request failed: {err}")));
        }
    };

    let v: serde_json::Value = response.into_json()?;
    let content = v
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| {
            LlmError::InvalidOutput(
                "OpenRouter response missing choices[0].message.content".to_string(),
            )
        })?;

    parse_llm_output_str(content)
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
            "--mcpcc-verbose",
            "--mcpcc-print-cc",
            "--",
            "-Wall",
            "hello.c",
        ]))
        .expect("parse");
        assert_eq!(parsed.wrapper.cc.as_deref(), Some("mycc"));
        assert!(parsed.wrapper.verbose);
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
            "--mcpcc-verbose",
            "--mcpcc-print-cc",
        ]))
        .expect("parse");
        assert_eq!(parsed.wrapper.cc.as_deref(), Some("clang"));
        assert!(parsed.wrapper.verbose);
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
    fn parse_args_errors_on_invalid_llm_mode() {
        let err =
            parse_args(&v(&["--mcpcc-llm-mode=wat"])).expect_err("invalid llm mode should fail");
        assert_eq!(
            err,
            CliParseError::InvalidValue {
                flag: "--mcpcc-llm-mode".to_string(),
                value: "wat".to_string(),
            }
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
