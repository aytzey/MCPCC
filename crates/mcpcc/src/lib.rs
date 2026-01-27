use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use object::{Object, ObjectSection};
use sha2::{Digest, Sha256};

pub const MCP_SPEC_VERSION: &str = "2025-11-25";
pub const LLM_PROMPT_VERSION: &str = "v2";
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
pub enum ServerCopyError {
    CurrentExe(std::io::Error),
    NotFound(Vec<PathBuf>),
    Io(std::io::Error),
}

impl std::fmt::Display for ServerCopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerCopyError::CurrentExe(err) => write!(f, "failed to resolve current exe: {err}"),
            ServerCopyError::NotFound(paths) => {
                if paths.is_empty() {
                    return write!(f, "mcpcc-mcp-server binary not found");
                }
                write!(f, "mcpcc-mcp-server binary not found; tried: ")?;
                for (idx, path) in paths.iter().enumerate() {
                    if idx > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", path.display())?;
                }
                Ok(())
            }
            ServerCopyError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ServerCopyError {}

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
pub struct LlmBundleDescriptions {
    pub tools: BTreeMap<String, LlmToolDescriptions>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionArgRequirement {
    None,
    Required,
    Optional,
}

impl OptionArgRequirement {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GetoptLongOptionSpec {
    long_name: String,
    long_arg: OptionArgRequirement,
    short: Option<char>,
    short_arg: Option<OptionArgRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GetoptLongSpec {
    options: Vec<GetoptLongOptionSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArgpOptionSpec {
    long_name: String,
    arg_requirement: OptionArgRequirement,
    short: Option<char>,
    doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArgpSpec {
    options: Vec<ArgpOptionSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct AnalysisSummary {
    pub used_libclang: bool,
    pub extractors: Vec<String>,
    pub structured_tool_generated: bool,
    pub param_count: usize,
    pub notes: Vec<String>,
}

pub struct McpJsonPlan {
    pub mcp_json: serde_json::Value,
    pub analysis: AnalysisSummary,
    pub llm_expected: BTreeMap<String, Vec<String>>,
    pub llm_summary_json: String,
    annotations: McpccAnnotations,
}

const MCPCC_ANNOT_SECTION: &str = ".mcpcc";
const MCPCC_TOOL_PREFIX: &str = "MCPCC_TOOL:";
const MCPCC_PARAM_PREFIX: &str = "MCPCC_PARAM:";
const TOOL_NAME_MAX_LEN: usize = 128;
const TOOL_RUN_RAW_SUFFIX: &str = ".run_raw";
const TOOL_BASE_MAX_LEN: usize = TOOL_NAME_MAX_LEN - TOOL_RUN_RAW_SUFFIX.len();

const DEFAULT_EXEC_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_EXEC_MAX_STDOUT_BYTES: u64 = 1_048_576;
const DEFAULT_EXEC_MAX_STDERR_BYTES: u64 = 1_048_576;

fn normalize_tool_base_name(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if out.len() >= TOOL_BASE_MAX_LEN {
            break;
        }
        if matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() {
        "tool".to_string()
    } else {
        out
    }
}

fn run_raw_tool_name_from_base(base: &str) -> String {
    format!("{base}{TOOL_RUN_RAW_SUFFIX}")
}

fn normalize_tool_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(base) = trimmed.strip_suffix(TOOL_RUN_RAW_SUFFIX) {
        run_raw_tool_name_from_base(&normalize_tool_base_name(base))
    } else {
        normalize_tool_base_name(trimmed)
    }
}

fn tool_base_name_for_artifacts(artifacts: &ArtifactPaths) -> String {
    normalize_tool_base_name(&artifacts.base_name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolAnnotation {
    name: String,
    title: Option<String>,
    description: Option<String>,
    timeout_ms: Option<u64>,
    max_stdout_bytes: Option<u64>,
    max_stderr_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParamAnnotation {
    tool: String,
    property: String,
    long: Option<String>,
    short: Option<String>,
    takes_value: Option<bool>,
    ty: Option<String>,
    repeatable: Option<bool>,
    required: Option<bool>,
    description: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct McpccAnnotations {
    tools: Vec<ToolAnnotation>,
    params: Vec<ParamAnnotation>,
    notes: Vec<String>,
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
    build_mcp_json(artifacts, descriptions, None)
}

fn default_tool_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "title": "mcpcc Tool Output",
        "description": "Structured output produced under `structuredContent` for mcpcc tools.",
        "properties": {
            "stdout": { "type": "string" },
            "stderr": { "type": "string" },
            "exitCode": { "type": "integer" },
            "durationMs": { "type": "integer" },
            "timedOut": { "type": "boolean" },
            "truncatedStdout": { "type": "boolean" },
            "truncatedStderr": { "type": "boolean" },
        },
        "required": [
            "stdout",
            "stderr",
            "exitCode",
            "durationMs",
            "timedOut",
            "truncatedStdout",
            "truncatedStderr",
        ],
        "additionalProperties": false,
    })
}

fn ensure_prd_tool_defaults(tool: &mut serde_json::Value, kind: &str, default_title: &str) {
    let Some(obj) = tool.as_object_mut() else {
        return;
    };

    let inferred_title = obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(default_title)
        .to_string();
    obj.entry("title".to_string())
        .or_insert_with(|| serde_json::Value::String(inferred_title));

    obj.entry("outputSchema".to_string())
        .or_insert_with(default_tool_output_schema);

    let input_schema = obj
        .entry("inputSchema".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !input_schema.is_object() {
        *input_schema = serde_json::Value::Object(serde_json::Map::new());
    }
    let input_obj = input_schema.as_object_mut().expect("inputSchema object");

    input_obj
        .entry("type".to_string())
        .or_insert_with(|| serde_json::Value::String("object".to_string()));
    let props = input_obj
        .entry("properties".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !props.is_object() {
        *props = serde_json::Value::Object(serde_json::Map::new());
    }
    input_obj.insert(
        "additionalProperties".to_string(),
        serde_json::Value::Bool(false),
    );

    let x_mcpcc = obj
        .entry("x-mcpcc".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !x_mcpcc.is_object() {
        *x_mcpcc = serde_json::Value::Object(serde_json::Map::new());
    }
    let x_obj = x_mcpcc.as_object_mut().expect("x-mcpcc object");
    x_obj
        .entry("kind".to_string())
        .or_insert_with(|| serde_json::Value::String(kind.to_string()));
}

fn build_run_raw_tool_json(
    artifacts: &ArtifactPaths,
    descriptions: &LlmToolDescriptions,
) -> serde_json::Value {
    let base_name = tool_base_name_for_artifacts(artifacts);
    let tool_name = run_raw_tool_name_from_base(&base_name);
    let argv_description = descriptions
        .params
        .get("argv")
        .map(String::as_str)
        .unwrap_or("Arguments to pass to the binary as an argv array.");

    let mut tool = serde_json::json!({
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
    });
    ensure_prd_tool_defaults(&mut tool, "raw", &tool_name);
    tool
}

fn build_getopt_long_structured_tool_json(
    artifacts: &ArtifactPaths,
    spec: &GetoptLongSpec,
) -> serde_json::Value {
    let base_name = tool_base_name_for_artifacts(artifacts);
    let mut properties = serde_json::Map::new();
    for opt in &spec.options {
        let schema = match opt.long_arg {
            OptionArgRequirement::None => serde_json::json!({ "type": "boolean" }),
            OptionArgRequirement::Required | OptionArgRequirement::Optional => {
                serde_json::json!({ "type": "string" })
            }
        };
        properties.insert(opt.long_name.clone(), schema);
    }
    properties.insert(
        "args".to_string(),
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
        }),
    );

    let mut mapping_options = Vec::with_capacity(spec.options.len());
    for (idx, opt) in spec.options.iter().enumerate() {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "property".to_string(),
            serde_json::Value::String(opt.long_name.clone()),
        );
        entry.insert(
            "long".to_string(),
            serde_json::Value::String(format!("--{}", opt.long_name)),
        );
        entry.insert(
            "takesValue".to_string(),
            serde_json::Value::Bool(!matches!(opt.long_arg, OptionArgRequirement::None)),
        );
        entry.insert(
            "valueStyle".to_string(),
            serde_json::Value::String("separate".to_string()),
        );
        entry.insert("repeatable".to_string(), serde_json::Value::Bool(false));
        entry.insert(
            "position".to_string(),
            serde_json::Value::Number(serde_json::Number::from(idx)),
        );
        if let Some(short) = opt.short {
            entry.insert(
                "short".to_string(),
                serde_json::Value::String(format!("-{short}")),
            );
        }
        mapping_options.push(serde_json::Value::Object(entry));
    }

    let mut tool = serde_json::json!({
        "name": base_name,
        "description": format!("Run {} with structured options.", base_name),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        },
        "x-mcpcc": {
            "argvMapping": {
                "options": mapping_options,
                "positionalProperty": "args",
            },
        },
    });
    ensure_prd_tool_defaults(&mut tool, "structured", &base_name);
    tool
}

fn build_argp_structured_tool_json(
    artifacts: &ArtifactPaths,
    spec: &ArgpSpec,
) -> serde_json::Value {
    let base_name = tool_base_name_for_artifacts(artifacts);
    let mut properties = serde_json::Map::new();
    for opt in &spec.options {
        let schema = match opt.arg_requirement {
            OptionArgRequirement::None => serde_json::json!({ "type": "boolean" }),
            OptionArgRequirement::Required | OptionArgRequirement::Optional => {
                serde_json::json!({ "type": "string" })
            }
        };
        let schema = if let Some(doc) = opt.doc.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty())
        {
            let mut obj = schema
                .as_object()
                .cloned()
                .unwrap_or_else(|| serde_json::Map::new());
            obj.insert(
                "description".to_string(),
                serde_json::Value::String(doc.to_string()),
            );
            serde_json::Value::Object(obj)
        } else {
            schema
        };
        properties.insert(opt.long_name.clone(), schema);
    }
    properties.insert(
        "args".to_string(),
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
        }),
    );

    let mut mapping_options = Vec::with_capacity(spec.options.len());
    for (idx, opt) in spec.options.iter().enumerate() {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "property".to_string(),
            serde_json::Value::String(opt.long_name.clone()),
        );
        entry.insert(
            "long".to_string(),
            serde_json::Value::String(format!("--{}", opt.long_name)),
        );
        entry.insert(
            "takesValue".to_string(),
            serde_json::Value::Bool(!matches!(opt.arg_requirement, OptionArgRequirement::None)),
        );
        entry.insert(
            "valueStyle".to_string(),
            serde_json::Value::String("separate".to_string()),
        );
        entry.insert("repeatable".to_string(), serde_json::Value::Bool(false));
        entry.insert(
            "position".to_string(),
            serde_json::Value::Number(serde_json::Number::from(idx)),
        );
        if let Some(short) = opt.short {
            entry.insert(
                "short".to_string(),
                serde_json::Value::String(format!("-{short}")),
            );
        }
        mapping_options.push(serde_json::Value::Object(entry));
    }

    let mut tool = serde_json::json!({
        "name": base_name,
        "description": format!("Run {} with structured options.", base_name),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        },
        "x-mcpcc": {
            "argvMapping": {
                "options": mapping_options,
                "positionalProperty": "args",
            },
        },
    });
    ensure_prd_tool_defaults(&mut tool, "structured", &base_name);
    tool
}

fn binary_path_for_mcp_json(bin_path: &Path) -> String {
    // `std::process::Command::new("foo")` searches PATH. It does NOT execute `./foo`.
    // Our server currently calls Command::new(binary.path) without setting cwd.
    // So for bare relative filenames like `calc`, we must emit `./calc`.
    if bin_path.is_absolute() {
        return bin_path.to_string_lossy().to_string();
    }

    // If it is a single path component ("calc"), prefix "./".
    if bin_path.components().count() == 1 {
        return format!("./{}", bin_path.to_string_lossy());
    }

    // If it has a parent but still no separators via Path rules, keep it.
    // (e.g. "bin/calc" should work as a relative path because it contains a separator)
    bin_path.to_string_lossy().to_string()
}

pub fn build_mcp_json(
    artifacts: &ArtifactPaths,
    descriptions: &LlmToolDescriptions,
    structured_tool: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut tools = Vec::new();
    if let Some(tool) = structured_tool {
        tools.push(tool);
    }
    tools.push(build_run_raw_tool_json(artifacts, descriptions));

    let mut mcp_json = serde_json::json!({
        "mcpccVersion": env!("CARGO_PKG_VERSION"),
        "mcpSpecVersion": MCP_SPEC_VERSION,
        "binary": {
            "path": binary_path_for_mcp_json(&artifacts.bin_path),
            "defaultCwd": serde_json::Value::Null,
        },
        "tools": tools,
    });

    ensure_default_exec_limits(&mut mcp_json);

    mcp_json
}

fn ensure_default_exec_limits(mcp_json: &mut serde_json::Value) {
    let Some(tools) = mcp_json.get_mut("tools").and_then(|v| v.as_array_mut()) else {
        return;
    };

    for tool in tools {
        let Some(obj) = tool.as_object_mut() else {
            continue;
        };

        let x_mcpcc = obj
            .entry("x-mcpcc".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !x_mcpcc.is_object() {
            *x_mcpcc = serde_json::Value::Object(serde_json::Map::new());
        }
        let x_obj = x_mcpcc.as_object_mut().expect("x-mcpcc object");

        let exec = x_obj
            .entry("exec".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !exec.is_object() {
            *exec = serde_json::Value::Object(serde_json::Map::new());
        }
        let exec_obj = exec.as_object_mut().expect("exec object");

        exec_obj.entry("timeoutMs".to_string()).or_insert_with(|| {
            serde_json::Value::Number(serde_json::Number::from(DEFAULT_EXEC_TIMEOUT_MS))
        });
        exec_obj
            .entry("maxStdoutBytes".to_string())
            .or_insert_with(|| {
                serde_json::Value::Number(serde_json::Number::from(DEFAULT_EXEC_MAX_STDOUT_BYTES))
            });
        exec_obj
            .entry("maxStderrBytes".to_string())
            .or_insert_with(|| {
                serde_json::Value::Number(serde_json::Number::from(DEFAULT_EXEC_MAX_STDERR_BYTES))
            });
    }
}

fn non_empty_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn non_empty_str_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    obj.get(key).and_then(non_empty_string)
}

fn optional_bool_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<bool>, String> {
    let Some(value) = obj.get(key) else {
        return Ok(None);
    };
    value
        .as_bool()
        .ok_or_else(|| format!("{key} must be a boolean"))
        .map(Some)
}

fn optional_u64_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<u64>, String> {
    let Some(value) = obj.get(key) else {
        return Ok(None);
    };

    if let Some(n) = value.as_u64() {
        return Ok(Some(n));
    }
    if let Some(n) = value.as_i64() {
        return if n >= 0 {
            Ok(Some(n as u64))
        } else {
            Err(format!("{key} must be non-negative"))
        };
    }

    Err(format!("{key} must be an integer"))
}

fn parse_tool_annotation_json(payload: &str) -> Result<ToolAnnotation, String> {
    let v: serde_json::Value =
        serde_json::from_str(payload).map_err(|err| format!("invalid JSON: {err}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "tool annotation must be a JSON object".to_string())?;

    let name_raw = obj
        .get("name")
        .and_then(non_empty_string)
        .ok_or_else(|| "tool annotation missing required string field: name".to_string())?;
    let name = normalize_tool_name(&name_raw);

    Ok(ToolAnnotation {
        name,
        title: non_empty_str_field(obj, "title"),
        description: non_empty_str_field(obj, "description"),
        timeout_ms: optional_u64_field(obj, "timeoutMs")?,
        max_stdout_bytes: optional_u64_field(obj, "maxStdoutBytes")?,
        max_stderr_bytes: optional_u64_field(obj, "maxStderrBytes")?,
    })
}

fn parse_param_annotation_json(payload: &str) -> Result<ParamAnnotation, String> {
    let v: serde_json::Value =
        serde_json::from_str(payload).map_err(|err| format!("invalid JSON: {err}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "param annotation must be a JSON object".to_string())?;

    let tool_raw = obj
        .get("tool")
        .and_then(non_empty_string)
        .ok_or_else(|| "param annotation missing required string field: tool".to_string())?;
    let tool = normalize_tool_name(&tool_raw);
    let property = obj
        .get("property")
        .and_then(non_empty_string)
        .ok_or_else(|| "param annotation missing required string field: property".to_string())?;

    let ty = non_empty_str_field(obj, "type").and_then(|ty| {
        matches!(ty.as_str(), "boolean" | "string" | "integer" | "number").then_some(ty)
    });
    if obj.get("type").is_some() && ty.is_none() {
        return Err("type must be one of: boolean|string|integer|number".to_string());
    }

    Ok(ParamAnnotation {
        tool,
        property,
        long: non_empty_str_field(obj, "long"),
        short: non_empty_str_field(obj, "short"),
        takes_value: optional_bool_field(obj, "takesValue")?,
        ty,
        repeatable: optional_bool_field(obj, "repeatable")?,
        required: optional_bool_field(obj, "required")?,
        description: non_empty_str_field(obj, "description"),
    })
}

fn parse_mcpcc_annotation_section(section_data: &[u8]) -> McpccAnnotations {
    let mut out = McpccAnnotations::default();

    for raw in section_data.split(|b| *b == 0) {
        if raw.is_empty() {
            continue;
        }
        let Ok(text) = std::str::from_utf8(raw) else {
            continue;
        };
        let text = text.trim();
        if let Some(payload) = text.strip_prefix(MCPCC_TOOL_PREFIX) {
            match parse_tool_annotation_json(payload) {
                Ok(annotation) => out.tools.push(annotation),
                Err(err) => out
                    .notes
                    .push(format!("annotation tool parse failed: {err}")),
            }
            continue;
        }
        if let Some(payload) = text.strip_prefix(MCPCC_PARAM_PREFIX) {
            match parse_param_annotation_json(payload) {
                Ok(annotation) => out.params.push(annotation),
                Err(err) => out
                    .notes
                    .push(format!("annotation param parse failed: {err}")),
            }
        }
    }

    out
}

fn read_mcpcc_annotations(bin_path: &Path) -> McpccAnnotations {
    let bytes = match std::fs::read(bin_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            return McpccAnnotations {
                notes: vec![format!(
                    "annotation read failed for {}: {err}",
                    bin_path.display()
                )],
                ..Default::default()
            };
        }
    };

    let file = match object::File::parse(&*bytes) {
        Ok(file) => file,
        Err(err) => {
            return McpccAnnotations {
                notes: vec![format!(
                    "annotation parse failed for {}: {err}",
                    bin_path.display()
                )],
                ..Default::default()
            };
        }
    };

    let Some(section) = file.section_by_name(MCPCC_ANNOT_SECTION) else {
        return McpccAnnotations::default();
    };

    let section_data = match section.data() {
        Ok(data) => data,
        Err(err) => {
            return McpccAnnotations {
                notes: vec![format!("annotation section read failed: {err}")],
                ..Default::default()
            };
        }
    };

    parse_mcpcc_annotation_section(section_data)
}

fn annotation_schema_type(annotation: &ParamAnnotation) -> Option<&'static str> {
    if let Some(ty) = annotation.ty.as_deref() {
        return Some(match ty {
            "boolean" => "boolean",
            "string" => "string",
            "integer" => "integer",
            "number" => "number",
            _ => return None,
        });
    }

    annotation
        .takes_value
        .map(|takes_value| if takes_value { "string" } else { "boolean" })
}

fn apply_tool_annotation(tool: &mut serde_json::Value, annotation: &ToolAnnotation) -> bool {
    let Some(obj) = tool.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    if let Some(title) = annotation.title.as_ref() {
        obj.insert(
            "title".to_string(),
            serde_json::Value::String(title.clone()),
        );
        changed = true;
    }
    if let Some(description) = annotation.description.as_ref() {
        obj.insert(
            "description".to_string(),
            serde_json::Value::String(description.clone()),
        );
        changed = true;
    }

    if annotation.timeout_ms.is_some()
        || annotation.max_stdout_bytes.is_some()
        || annotation.max_stderr_bytes.is_some()
    {
        let x_mcpcc = obj
            .entry("x-mcpcc".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !x_mcpcc.is_object() {
            *x_mcpcc = serde_json::Value::Object(serde_json::Map::new());
        }
        let x_obj = x_mcpcc.as_object_mut().expect("x-mcpcc object");
        let exec = x_obj
            .entry("exec".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !exec.is_object() {
            *exec = serde_json::Value::Object(serde_json::Map::new());
        }
        let exec_obj = exec.as_object_mut().expect("exec object");

        if let Some(timeout_ms) = annotation.timeout_ms {
            exec_obj.insert(
                "timeoutMs".to_string(),
                serde_json::Value::Number(serde_json::Number::from(timeout_ms)),
            );
            changed = true;
        }
        if let Some(max_stdout_bytes) = annotation.max_stdout_bytes {
            exec_obj.insert(
                "maxStdoutBytes".to_string(),
                serde_json::Value::Number(serde_json::Number::from(max_stdout_bytes)),
            );
            changed = true;
        }
        if let Some(max_stderr_bytes) = annotation.max_stderr_bytes {
            exec_obj.insert(
                "maxStderrBytes".to_string(),
                serde_json::Value::Number(serde_json::Number::from(max_stderr_bytes)),
            );
            changed = true;
        }
    }

    changed
}

fn apply_param_annotation(tool: &mut serde_json::Value, annotation: &ParamAnnotation) -> bool {
    let Some(obj) = tool.as_object_mut() else {
        return false;
    };

    let Some(input_schema) = obj.get_mut("inputSchema").and_then(|v| v.as_object_mut()) else {
        return false;
    };
    let properties = input_schema
        .entry("properties".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !properties.is_object() {
        *properties = serde_json::Value::Object(serde_json::Map::new());
    }
    let props_obj = properties.as_object_mut().expect("properties object");

    let entry = props_obj
        .entry(annotation.property.clone())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = serde_json::Value::Object(serde_json::Map::new());
    }
    let schema_obj = entry.as_object_mut().expect("schema object");

    let mut changed = false;
    if let Some(schema_type) = annotation_schema_type(annotation) {
        schema_obj.insert(
            "type".to_string(),
            serde_json::Value::String(schema_type.to_string()),
        );
        changed = true;
    }
    if let Some(description) = annotation.description.as_ref() {
        schema_obj.insert(
            "description".to_string(),
            serde_json::Value::String(description.clone()),
        );
        changed = true;
    }

    if annotation.required == Some(true) {
        let required = input_schema
            .entry("required".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if !required.is_array() {
            *required = serde_json::Value::Array(Vec::new());
        }
        let required_arr = required.as_array_mut().expect("required array");
        if !required_arr
            .iter()
            .any(|v| v.as_str() == Some(&annotation.property))
        {
            required_arr.push(serde_json::Value::String(annotation.property.clone()));
            changed = true;
        }
    }

    let Some(options) = obj
        .get_mut("x-mcpcc")
        .and_then(|v| v.get_mut("argvMapping"))
        .and_then(|v| v.get_mut("options"))
        .and_then(|v| v.as_array_mut())
    else {
        return changed;
    };

    let mut option_obj: Option<&mut serde_json::Map<String, serde_json::Value>> = None;
    for opt in options.iter_mut() {
        let Some(opt_map) = opt.as_object_mut() else {
            continue;
        };
        let matches = opt_map
            .get("property")
            .or_else(|| opt_map.get("param"))
            .and_then(|v| v.as_str())
            == Some(annotation.property.as_str());
        if matches {
            option_obj = Some(opt_map);
            break;
        }
    }

    if option_obj.is_none() {
        let mut new = serde_json::Map::new();
        new.insert(
            "property".to_string(),
            serde_json::Value::String(annotation.property.clone()),
        );
        let long = annotation
            .long
            .clone()
            .unwrap_or_else(|| format!("--{}", annotation.property));
        new.insert("long".to_string(), serde_json::Value::String(long));
        if let Some(short) = annotation.short.clone() {
            new.insert("short".to_string(), serde_json::Value::String(short));
        }
        let schema_type = annotation_schema_type(annotation).unwrap_or("string");
        let takes_value = annotation
            .takes_value
            .unwrap_or_else(|| schema_type != "boolean");
        new.insert(
            "takesValue".to_string(),
            serde_json::Value::Bool(takes_value),
        );
        new.insert(
            "valueStyle".to_string(),
            serde_json::Value::String("separate".to_string()),
        );
        new.insert(
            "repeatable".to_string(),
            serde_json::Value::Bool(annotation.repeatable.unwrap_or(false)),
        );
        options.push(serde_json::Value::Object(new));
        return true;
    }

    let opt_map = option_obj.expect("option object");
    opt_map.insert(
        "property".to_string(),
        serde_json::Value::String(annotation.property.clone()),
    );
    if let Some(long) = annotation.long.as_ref() {
        opt_map.insert("long".to_string(), serde_json::Value::String(long.clone()));
        changed = true;
    }
    if let Some(short) = annotation.short.as_ref() {
        opt_map.insert(
            "short".to_string(),
            serde_json::Value::String(short.clone()),
        );
        changed = true;
    }
    if let Some(repeatable) = annotation.repeatable {
        opt_map.insert(
            "repeatable".to_string(),
            serde_json::Value::Bool(repeatable),
        );
        changed = true;
    }

    if let Some(takes_value) = annotation.takes_value {
        opt_map.insert(
            "takesValue".to_string(),
            serde_json::Value::Bool(takes_value),
        );
        changed = true;
    }

    changed
}

fn apply_annotations_to_mcp_json(
    mcp_json: &mut serde_json::Value,
    annotations: &McpccAnnotations,
) -> bool {
    let Some(tools) = mcp_json.get_mut("tools").and_then(|v| v.as_array_mut()) else {
        return false;
    };

    let mut used = false;
    for tool in tools.iter_mut() {
        let Some(name) = tool
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        else {
            continue;
        };

        for annotation in annotations.tools.iter().filter(|a| a.name == name) {
            used |= apply_tool_annotation(tool, annotation);
        }
        for annotation in annotations.params.iter().filter(|a| a.tool == name) {
            used |= apply_param_annotation(tool, annotation);
        }
    }

    used
}

fn build_annotation_structured_tool_json(
    artifacts: &ArtifactPaths,
    params: &[ParamAnnotation],
) -> serde_json::Value {
    let base_name = tool_base_name_for_artifacts(artifacts);
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    let mut mapping_options = Vec::new();
    for (idx, annotation) in params.iter().enumerate() {
        let Some(schema_type) = annotation_schema_type(annotation).or(Some("string")) else {
            continue;
        };
        let mut schema = serde_json::Map::new();
        schema.insert(
            "type".to_string(),
            serde_json::Value::String(schema_type.to_string()),
        );
        if let Some(description) = annotation.description.as_ref() {
            schema.insert(
                "description".to_string(),
                serde_json::Value::String(description.clone()),
            );
        }
        properties.insert(
            annotation.property.clone(),
            serde_json::Value::Object(schema),
        );

        if annotation.required == Some(true) {
            required.push(serde_json::Value::String(annotation.property.clone()));
        }

        let mut entry = serde_json::Map::new();
        entry.insert(
            "property".to_string(),
            serde_json::Value::String(annotation.property.clone()),
        );
        let long = annotation
            .long
            .clone()
            .unwrap_or_else(|| format!("--{}", annotation.property));
        entry.insert("long".to_string(), serde_json::Value::String(long));

        if let Some(short) = annotation.short.clone() {
            entry.insert("short".to_string(), serde_json::Value::String(short));
        }
        entry.insert(
            "takesValue".to_string(),
            serde_json::Value::Bool(annotation.takes_value.unwrap_or(schema_type != "boolean")),
        );
        entry.insert(
            "valueStyle".to_string(),
            serde_json::Value::String("separate".to_string()),
        );
        entry.insert(
            "repeatable".to_string(),
            serde_json::Value::Bool(annotation.repeatable.unwrap_or(false)),
        );
        entry.insert(
            "position".to_string(),
            serde_json::Value::Number(serde_json::Number::from(idx)),
        );
        mapping_options.push(serde_json::Value::Object(entry));
    }

    properties.insert(
        "args".to_string(),
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
        }),
    );

    let mut input_schema = serde_json::Map::new();
    input_schema.insert(
        "type".to_string(),
        serde_json::Value::String("object".to_string()),
    );
    input_schema.insert(
        "properties".to_string(),
        serde_json::Value::Object(properties),
    );
    input_schema.insert(
        "additionalProperties".to_string(),
        serde_json::Value::Bool(false),
    );
    if !required.is_empty() {
        input_schema.insert("required".to_string(), serde_json::Value::Array(required));
    }

    let mut tool = serde_json::json!({
        "name": base_name,
        "description": format!("Run {} with structured options.", base_name),
        "inputSchema": serde_json::Value::Object(input_schema),
        "x-mcpcc": {
            "argvMapping": {
                "options": mapping_options,
                "positionalProperty": "args",
            },
        },
    });
    ensure_prd_tool_defaults(&mut tool, "structured", &base_name);
    tool
}

fn count_tool_param_count(mcp_json: &serde_json::Value, tool_name: &str) -> Option<usize> {
    let tools = mcp_json.get("tools")?.as_array()?;
    let tool = tools
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some(tool_name))?;
    let props = tool.get("inputSchema")?.get("properties")?.as_object()?;
    Some(props.len())
}

pub fn write_mcp_json_atomic(
    artifacts: &ArtifactPaths,
    plan: &McpJsonPlan,
) -> Result<(), JsonWriteError> {
    write_json_atomic(&artifacts.mcp_json_path, &plan.mcp_json)?;
    Ok(())
}

fn llm_expected_from_mcp_json(mcp_json: &serde_json::Value) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(tools) = mcp_json.get("tools").and_then(|v| v.as_array()) else {
        return out;
    };

    for tool in tools {
        let Some(tool_name) = tool.get("name").and_then(|v| v.as_str()) else {
            continue;
        };

        let mut params = Vec::<String>::new();
        if let Some(mapping) = tool
            .get("x-mcpcc")
            .and_then(|v| v.get("argvMapping"))
            .and_then(|v| v.as_object())
        {
            if let Some(options) = mapping.get("options").and_then(|v| v.as_array()) {
                for opt in options {
                    let Some(opt_obj) = opt.as_object() else {
                        continue;
                    };
                    let Some(prop) = opt_obj
                        .get("property")
                        .or_else(|| opt_obj.get("param"))
                        .and_then(|v| v.as_str())
                    else {
                        continue;
                    };
                    params.push(prop.to_string());
                }
            }
            let positional = mapping
                .get("positionalProperty")
                .or_else(|| mapping.get("argsParam"))
                .and_then(|v| v.as_str())
                .unwrap_or("args");
            if !params.iter().any(|p| p == positional) {
                params.push(positional.to_string());
            }
        } else if let Some(props) = tool
            .get("inputSchema")
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.as_object())
        {
            params.extend(props.keys().cloned());
            params.sort();
            if let Some(args_idx) = params.iter().position(|p| p == "args") {
                let args = params.remove(args_idx);
                params.push(args);
            }
        }

        out.insert(tool_name.to_string(), params);
    }

    out
}

fn guessed_type_from_schema(schema: &serde_json::Value) -> String {
    let Some(obj) = schema.as_object() else {
        return "unknown".to_string();
    };
    let Some(ty) = obj.get("type").and_then(|v| v.as_str()) else {
        return "unknown".to_string();
    };
    if ty == "array" {
        let item_ty = obj
            .get("items")
            .and_then(|v| v.as_object())
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return format!("array<{item_ty}>");
    }
    ty.to_string()
}

fn truncate_llm_doc(value: &str) -> String {
    const MAX: usize = 160;
    let trimmed = value.trim();
    let mut out = String::new();
    let mut count = 0usize;
    for ch in trimmed.chars() {
        if count >= MAX {
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn llm_bundle_analysis_summary_json(artifacts: &ArtifactPaths, plan: &McpJsonPlan) -> String {
    let binary_name = tool_base_name_for_artifacts(artifacts);

    let mut tools_summary = Vec::new();
    let tools = plan
        .mcp_json
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for tool in tools {
        let Some(tool_name) = tool.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(expected_params) = plan.llm_expected.get(tool_name) else {
            continue;
        };

        let mut mapping_by_property: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        if let Some(options) = tool
            .get("x-mcpcc")
            .and_then(|v| v.get("argvMapping"))
            .and_then(|v| v.get("options"))
            .and_then(|v| v.as_array())
        {
            for opt in options {
                let Some(opt_obj) = opt.as_object() else {
                    continue;
                };
                let Some(prop) = opt_obj
                    .get("property")
                    .or_else(|| opt_obj.get("param"))
                    .and_then(|v| v.as_str())
                else {
                    continue;
                };
                mapping_by_property.insert(prop.to_string(), opt.clone());
            }
        }

        let props = tool
            .get("inputSchema")
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.as_object());

        let mut params_summary = Vec::new();
        for param in expected_params {
            let schema = props
                .and_then(|p| p.get(param))
                .cloned()
                .unwrap_or_default();
            let guessed_type = guessed_type_from_schema(&schema);
            let doc = schema
                .get("description")
                .and_then(|v| v.as_str())
                .map(truncate_llm_doc);

            let (long, short, takes_value) = mapping_by_property
                .get(param)
                .and_then(|v| v.as_object())
                .map(|opt| {
                    let long = opt.get("long").and_then(|v| v.as_str()).map(str::to_string);
                    let short = opt
                        .get("short")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let takes_value = opt
                        .get("takesValue")
                        .and_then(|v| v.as_bool())
                        .unwrap_or_else(|| guessed_type != "boolean");
                    (long, short, takes_value)
                })
                .unwrap_or_else(|| {
                    let takes_value = guessed_type != "boolean";
                    (None, None, takes_value)
                });

            params_summary.push(serde_json::json!({
                "property": param,
                "long": long,
                "short": short,
                "takesValue": takes_value,
                "optionalArg": takes_value,
                "guessedType": guessed_type,
                "doc": doc.unwrap_or_default(),
            }));
        }

        tools_summary.push(serde_json::json!({
            "toolName": tool_name,
            "binaryName": binary_name,
            "params": params_summary,
        }));
    }

    serde_json::json!({
        "binaryName": binary_name,
        "tools": tools_summary,
    })
    .to_string()
}

fn tool_annotation_overrides_description(annotations: &McpccAnnotations, tool: &str) -> bool {
    annotations.tools.iter().any(|a| {
        a.name == tool
            && a.description
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .is_some()
    })
}

fn param_annotation_overrides_description(
    annotations: &McpccAnnotations,
    tool: &str,
    property: &str,
) -> bool {
    annotations.params.iter().any(|a| {
        a.tool == tool
            && a.property == property
            && a.description
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .is_some()
    })
}

impl McpJsonPlan {
    pub fn apply_llm_descriptions(&mut self, descriptions: &LlmBundleDescriptions) {
        let Some(tools) = self
            .mcp_json
            .get_mut("tools")
            .and_then(|v| v.as_array_mut())
        else {
            return;
        };

        for tool in tools {
            let Some(tool_name) = tool
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            else {
                continue;
            };
            let Some(desc) = descriptions.tools.get(tool_name.as_str()) else {
                continue;
            };

            if !tool_annotation_overrides_description(&self.annotations, tool_name.as_str()) {
                if let Some(obj) = tool.as_object_mut() {
                    obj.insert(
                        "description".to_string(),
                        serde_json::Value::String(desc.tool_description.clone()),
                    );
                }
            }

            let Some(props) = tool
                .get_mut("inputSchema")
                .and_then(|v| v.get_mut("properties"))
                .and_then(|v| v.as_object_mut())
            else {
                continue;
            };

            let overwrite_param_descriptions = tool_name.ends_with(TOOL_RUN_RAW_SUFFIX);
            for (param, param_desc) in &desc.params {
                if param_annotation_overrides_description(
                    &self.annotations,
                    tool_name.as_str(),
                    param,
                ) {
                    continue;
                }
                let entry = props
                    .entry(param.clone())
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                if !entry.is_object() {
                    *entry = serde_json::Value::Object(serde_json::Map::new());
                }
                let schema_obj = entry.as_object_mut().expect("schema object");
                if !overwrite_param_descriptions {
                    if schema_obj
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .is_some()
                    {
                        continue;
                    }
                }
                schema_obj.insert(
                    "description".to_string(),
                    serde_json::Value::String(param_desc.clone()),
                );
            }
        }
    }
}

pub fn plan_mcp_json(
    artifacts: &ArtifactPaths,
    passthrough: &[String],
) -> Result<McpJsonPlan, JsonWriteError> {
    let (argp_spec, mut notes) = try_extract_argp_spec(passthrough);
    let (getopt_long_spec, getopt_long_notes) = if argp_spec.is_some() {
        (None, Vec::new())
    } else {
        try_extract_getopt_long_spec(passthrough)
    };
    notes.extend(getopt_long_notes);

    let annotations = read_mcpcc_annotations(&artifacts.bin_path);
    notes.extend(annotations.notes.iter().cloned());
    let tool_base_name = tool_base_name_for_artifacts(artifacts);

    let mut analysis = AnalysisSummary::default();
    analysis.notes = notes;

    let mut structured_tool = if let Some(spec) = argp_spec.as_ref() {
        analysis.extractors = vec!["argp".to_string()];
        analysis.structured_tool_generated = true;
        analysis.param_count = spec.options.len() + 1;
        Some(build_argp_structured_tool_json(artifacts, spec))
    } else if let Some(spec) = getopt_long_spec.as_ref() {
        analysis.extractors = vec!["getopt_long".to_string()];
        analysis.structured_tool_generated = true;
        analysis.param_count = spec.options.len() + 1;
        Some(build_getopt_long_structured_tool_json(artifacts, spec))
    } else {
        None
    };

    if structured_tool.is_none() {
        let param_annotations: Vec<ParamAnnotation> = annotations
            .params
            .iter()
            .filter(|a| a.tool == tool_base_name)
            .cloned()
            .collect();
        if !param_annotations.is_empty() {
            structured_tool = Some(build_annotation_structured_tool_json(
                artifacts,
                &param_annotations,
            ));
            analysis.structured_tool_generated = true;
            analysis.param_count = param_annotations.len() + 1;
            analysis.extractors = vec!["annotation".to_string()];
        }
    }

    let placeholder = placeholder_run_raw_descriptions();
    let mut mcp_json = build_mcp_json(artifacts, &placeholder, structured_tool);
    let used_annotations = apply_annotations_to_mcp_json(&mut mcp_json, &annotations);
    if used_annotations && !analysis.extractors.iter().any(|e| e == "annotation") {
        analysis.extractors.insert(0, "annotation".to_string());
    }
    if analysis.structured_tool_generated {
        if let Some(param_count) = count_tool_param_count(&mcp_json, &tool_base_name) {
            analysis.param_count = param_count;
        }
    }

    let llm_expected = llm_expected_from_mcp_json(&mcp_json);

    let mut plan = McpJsonPlan {
        mcp_json,
        analysis,
        llm_expected,
        llm_summary_json: String::new(),
        annotations,
    };
    plan.llm_summary_json = llm_bundle_analysis_summary_json(artifacts, &plan);

    Ok(plan)
}

pub fn build_manifest_json(
    compiler: &Path,
    compiler_args: &[String],
    compiler_exit_code: i32,
    artifacts: &ArtifactPaths,
    analysis: &AnalysisSummary,
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
            "usedLibclang": analysis.used_libclang,
            "extractors": analysis.extractors.clone(),
            "structuredToolGenerated": analysis.structured_tool_generated,
            "paramCount": analysis.param_count,
            "notes": analysis.notes.clone(),
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
    analysis: &AnalysisSummary,
    llm: &LlmManifestInfo,
) -> Result<(), JsonWriteError> {
    let manifest = build_manifest_json(
        compiler,
        compiler_args,
        compiler_exit_code,
        artifacts,
        analysis,
        llm,
    );
    write_json_atomic(&artifacts.manifest_path, &manifest)?;
    Ok(())
}

pub fn generate_llm_descriptions(
    wrapper: &WrapperFlags,
    llm_env: &LlmEnv,
    analysis_summary_json: &str,
    expected: &BTreeMap<String, Vec<String>>,
) -> Result<(LlmBundleDescriptions, LlmManifestInfo), LlmError> {
    let provider = "openrouter".to_string();
    let mode = wrapper.llm_mode;
    let model = resolve_llm_model(wrapper);
    let prompt_version = LLM_PROMPT_VERSION.to_string();
    let placeholder = placeholder_bundle_descriptions(expected);

    if mode == LlmMode::Off {
        return Ok((
            placeholder,
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

    let cache_dir = resolve_cache_dir(wrapper);
    let cache_key = llm_cache_key_hex(LLM_PROMPT_VERSION, &model, analysis_summary_json);
    let cache_path = cache_dir.join("llm").join(format!("{cache_key}.json"));

    if let Ok(descriptions) = read_llm_cache(&cache_path, expected) {
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
                placeholder,
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

    match call_openrouter(api_key, base_url, &model, analysis_summary_json, expected) {
        Ok(descriptions) => {
            let mut tools_obj = serde_json::Map::new();
            for (tool_name, tool_desc) in &descriptions.tools {
                tools_obj.insert(
                    tool_name.clone(),
                    serde_json::json!({
                        "toolDescription": tool_desc.tool_description.clone(),
                        "params": tool_desc.params.clone(),
                    }),
                );
            }
            let cache_value = serde_json::json!({ "tools": tools_obj });
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
                placeholder,
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

fn placeholder_bundle_descriptions(
    expected: &BTreeMap<String, Vec<String>>,
) -> LlmBundleDescriptions {
    let mut tools = BTreeMap::new();

    for (tool_name, params) in expected {
        let mut param_desc = BTreeMap::new();
        for param in params {
            let desc = match param.as_str() {
                "argv" => "Arguments to pass to the binary as an argv array.",
                "args" => "Positional arguments to pass after options.",
                other => {
                    if tool_name.ends_with(TOOL_RUN_RAW_SUFFIX) {
                        "Tool parameter."
                    } else {
                        // Keep this short; placeholders are only used for best-effort/off.
                        // Include the name so it's more informative.
                        return_desc_for_placeholder(other)
                    }
                }
            };
            param_desc.insert(param.clone(), desc.to_string());
        }

        let tool_description = if tool_name.ends_with(TOOL_RUN_RAW_SUFFIX) {
            "Run the target binary with raw argv and return stdout/stderr/exit code.".to_string()
        } else {
            "Run the target binary with structured options and return stdout/stderr/exit code."
                .to_string()
        };

        tools.insert(
            tool_name.clone(),
            LlmToolDescriptions {
                tool_description,
                params: param_desc,
            },
        );
    }

    LlmBundleDescriptions { tools }
}

fn return_desc_for_placeholder(param: &str) -> &'static str {
    // A tiny set of better defaults without inflating output.
    match param {
        "help" => "Show help/usage information.",
        "verbose" => "Enable verbose output.",
        _ => "Tool option value.",
    }
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

fn parse_bundle_llm_output(
    value: &serde_json::Value,
    expected: &BTreeMap<String, Vec<String>>,
) -> Result<LlmBundleDescriptions, LlmError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LlmError::InvalidOutput("LLM output must be a JSON object".to_string()))?;

    let tools_obj = obj
        .get("tools")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LlmError::InvalidOutput("LLM output missing tools object".to_string()))?;

    let mut tools = BTreeMap::new();
    for (tool_name, expected_params) in expected {
        let tool_val = tools_obj.get(tool_name).ok_or_else(|| {
            LlmError::InvalidOutput(format!("LLM output missing tools.{tool_name} object"))
        })?;
        let tool_obj = tool_val.as_object().ok_or_else(|| {
            LlmError::InvalidOutput(format!("tools.{tool_name} must be a JSON object"))
        })?;

        let tool_description_raw = tool_obj
            .get("toolDescription")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                LlmError::InvalidOutput(format!("tools.{tool_name}.toolDescription missing string"))
            })?;
        let Some(tool_description) = sanitize_description(tool_description_raw) else {
            return Err(LlmError::InvalidOutput(format!(
                "tools.{tool_name}.toolDescription must be 5–240 characters"
            )));
        };

        let params_obj = tool_obj
            .get("params")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                LlmError::InvalidOutput(format!("tools.{tool_name}.params missing object"))
            })?;

        let mut params = BTreeMap::new();
        for param in expected_params {
            let raw = params_obj
                .get(param)
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    LlmError::InvalidOutput(format!(
                        "tools.{tool_name}.params.{param} missing string"
                    ))
                })?;
            let Some(desc) = sanitize_description(raw) else {
                return Err(LlmError::InvalidOutput(format!(
                    "tools.{tool_name}.params.{param} must be 5–240 characters"
                )));
            };
            params.insert(param.clone(), desc);
        }

        tools.insert(
            tool_name.clone(),
            LlmToolDescriptions {
                tool_description,
                params,
            },
        );
    }

    Ok(LlmBundleDescriptions { tools })
}

fn parse_llm_output_str(
    content: &str,
    expected: &BTreeMap<String, Vec<String>>,
) -> Result<LlmBundleDescriptions, LlmError> {
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

    parse_bundle_llm_output(&parsed, expected)
}

fn read_llm_cache(
    path: &Path,
    expected: &BTreeMap<String, Vec<String>>,
) -> Result<LlmBundleDescriptions, LlmError> {
    let bytes = std::fs::read(path)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    parse_bundle_llm_output(&value, expected)
}

fn call_openrouter(
    api_key: &str,
    base_url: &str,
    model: &str,
    analysis_summary_json: &str,
    expected: &BTreeMap<String, Vec<String>>,
) -> Result<LlmBundleDescriptions, LlmError> {
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{base_url}/chat/completions");

    let system_prompt = concat!(
        "You generate short plain-text descriptions for an MCP tool bundle. ",
        "Return ONLY a JSON object with key: tools (object). ",
        "tools maps toolName to an object with keys: toolDescription (string) and params (object). ",
        "Include every toolName from analysis_summary_json and every listed param. ",
        "Output only those tools/params (no extras). ",
        "Do not use markdown or code fences. ",
        "All descriptions must be 5–240 characters after trimming.",
    );
    let user_prompt = format!(
        "analysis_summary_json:\n{analysis_summary_json}\n\nReturn JSON: \
{{\"tools\":{{\"toolName\":{{\"toolDescription\":\"...\",\"params\":{{\"param\":\"...\"}}}}}}}}"
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

    parse_llm_output_str(content, expected)
}

fn strip_c_comments(input: &str) -> String {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        LineComment,
        BlockComment,
        String,
        Char,
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut state = State::Normal;
    let mut escape = false;

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => {
                if ch == '/' {
                    match chars.peek().copied() {
                        Some('/') => {
                            let _ = chars.next();
                            out.push(' ');
                            out.push(' ');
                            state = State::LineComment;
                            continue;
                        }
                        Some('*') => {
                            let _ = chars.next();
                            out.push(' ');
                            out.push(' ');
                            state = State::BlockComment;
                            continue;
                        }
                        _ => {}
                    }
                }

                if ch == '"' {
                    state = State::String;
                    escape = false;
                    out.push(ch);
                    continue;
                }

                if ch == '\'' {
                    state = State::Char;
                    escape = false;
                    out.push(ch);
                    continue;
                }

                out.push(ch);
            }
            State::LineComment => {
                if ch == '\n' {
                    out.push('\n');
                    state = State::Normal;
                } else {
                    out.push(' ');
                }
            }
            State::BlockComment => {
                if ch == '*' && matches!(chars.peek().copied(), Some('/')) {
                    let _ = chars.next();
                    out.push(' ');
                    out.push(' ');
                    state = State::Normal;
                    continue;
                }
                if ch == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            State::String => {
                out.push(ch);
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '"' {
                    state = State::Normal;
                }
            }
            State::Char => {
                out.push(ch);
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '\'' {
                    state = State::Normal;
                }
            }
        }
    }

    out
}

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if in_char {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '\'' {
                in_char = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }

        if ch == separator && paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
            parts.push(input[start..idx].trim().to_string());
            start = idx + separator.len_utf8();
        }
    }

    parts.push(input[start..].trim().to_string());
    parts
}

fn find_matching_delimiter(input: &str, open_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;

    for (idx, ch) in input.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        if idx == open_idx {
            if ch != open {
                return None;
            }
            depth = 1;
            continue;
        }

        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if in_char {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '\'' {
                in_char = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_c_identifier(expr: &str) -> Option<String> {
    let expr = expr.trim();
    let expr = expr.strip_prefix('&').unwrap_or(expr).trim();
    let expr = expr.strip_prefix('(').unwrap_or(expr).trim();

    let mut chars = expr.chars().peekable();
    let first = chars.peek().copied()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }

    let mut out = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            out.push(ch);
            let _ = chars.next();
        } else {
            break;
        }
    }

    (!out.is_empty()).then_some(out)
}

fn decode_c_escape_sequence(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    let ch = chars.next()?;
    match ch {
        '\\' => Some('\\'),
        '\'' => Some('\''),
        '"' => Some('"'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        '0' => Some('\0'),
        'x' => {
            let mut value: u32 = 0;
            let mut count = 0;
            while let Some(next) = chars.peek().copied() {
                let digit = next.to_digit(16)?;
                value = (value << 4) | digit;
                count += 1;
                let _ = chars.next();
                if count >= 2 {
                    break;
                }
            }
            char::from_u32(value)
        }
        '1'..='7' => {
            let mut value: u32 = ch.to_digit(8)?;
            let mut count = 1;
            while count < 3 {
                let Some(next) = chars.peek().copied() else {
                    break;
                };
                let Some(digit) = next.to_digit(8) else {
                    break;
                };
                value = (value << 3) | digit;
                count += 1;
                let _ = chars.next();
            }
            char::from_u32(value)
        }
        other => Some(other),
    }
}

fn decode_c_string_contents(contents: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = contents.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let decoded = decode_c_escape_sequence(&mut chars)?;
            out.push(decoded);
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn parse_c_string_literal_expr(expr: &str) -> Option<String> {
    let mut rest = expr.trim();
    let mut out = String::new();
    let mut consumed_any = false;

    while rest.starts_with('"') {
        consumed_any = true;
        rest = &rest[1..];

        let mut end_idx = None;
        let mut escape = false;
        for (idx, ch) in rest.char_indices() {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                end_idx = Some(idx);
                break;
            }
        }

        let end_idx = end_idx?;
        let literal_contents = &rest[..end_idx];
        out.push_str(&decode_c_string_contents(literal_contents)?);
        rest = &rest[(end_idx + 1)..];
        rest = rest.trim_start();
    }

    (consumed_any && rest.is_empty()).then_some(out)
}

fn parse_c_char_literal(expr: &str) -> Option<char> {
    let expr = expr.trim();
    if !expr.starts_with('\'') || !expr.ends_with('\'') || expr.len() < 2 {
        return None;
    }
    let inner = &expr[1..(expr.len() - 1)];
    let decoded = decode_c_string_contents(inner)?;
    let mut chars = decoded.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(ch)
}

fn parse_getopt_long_call(source: &str) -> Option<(String, String)> {
    const NAME: &str = "getopt_long";
    for (idx, _) in source.match_indices(NAME) {
        let before_ok = idx == 0
            || !source[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric());
        if !before_ok {
            continue;
        }
        let after_idx = idx + NAME.len();
        let after_ok = after_idx == source.len()
            || !source[after_idx..]
                .chars()
                .next()
                .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric());
        if !after_ok {
            continue;
        }

        let mut cursor = after_idx;
        while let Some(ch) = source[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        if !source[cursor..].starts_with('(') {
            continue;
        }

        let close = find_matching_delimiter(source, cursor, '(', ')')?;
        let inner = &source[(cursor + 1)..close];
        let args = split_top_level(inner, ',');
        if args.len() < 4 {
            continue;
        }

        let optstring = parse_c_string_literal_expr(&args[2])?;
        let long_options_ident = parse_c_identifier(&args[3])?;
        return Some((optstring, long_options_ident));
    }
    None
}

fn parse_has_arg(value: &str) -> Option<OptionArgRequirement> {
    let value = value.trim().trim_matches(|c| c == '(' || c == ')').trim();

    match value {
        "no_argument" => return Some(OptionArgRequirement::None),
        "required_argument" => return Some(OptionArgRequirement::Required),
        "optional_argument" => return Some(OptionArgRequirement::Optional),
        _ => {}
    }

    let parsed: i64 = value.parse().ok()?;
    match parsed {
        0 => Some(OptionArgRequirement::None),
        1 => Some(OptionArgRequirement::Required),
        2 => Some(OptionArgRequirement::Optional),
        _ => None,
    }
}

fn parse_optstring(optstring: &str) -> BTreeMap<char, OptionArgRequirement> {
    let mut out = BTreeMap::new();
    let mut chars = optstring.chars().peekable();

    while matches!(chars.peek().copied(), Some(':' | '+' | '-')) {
        let _ = chars.next();
    }

    while let Some(ch) = chars.next() {
        if ch == ':' {
            continue;
        }

        let mut req = OptionArgRequirement::None;
        if matches!(chars.peek().copied(), Some(':')) {
            let _ = chars.next();
            req = OptionArgRequirement::Required;
            if matches!(chars.peek().copied(), Some(':')) {
                let _ = chars.next();
                req = OptionArgRequirement::Optional;
            }
        }
        out.insert(ch, req);
    }

    out
}

fn parse_struct_option_array(
    source: &str,
    ident: &str,
) -> Result<Vec<GetoptLongOptionSpec>, String> {
    const NEEDLE: &str = "struct option";
    for (idx, _) in source.match_indices(NEEDLE) {
        let after = idx + NEEDLE.len();
        let mut cursor = after;
        while let Some(ch) = source[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        if source[cursor..].starts_with('*') {
            cursor += 1;
            while let Some(ch) = source[cursor..].chars().next() {
                if ch.is_whitespace() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
        }

        if !source[cursor..].starts_with(ident) {
            continue;
        }
        let after_ident = cursor + ident.len();
        if source[after_ident..]
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            continue;
        }

        let eq_pos = source[after_ident..]
            .find('=')
            .map(|p| after_ident + p)
            .ok_or_else(|| format!("struct option {ident} missing '='"))?;
        let brace_pos = source[eq_pos..]
            .find('{')
            .map(|p| eq_pos + p)
            .ok_or_else(|| format!("struct option {ident} missing initializer"))?;

        let close = find_matching_delimiter(source, brace_pos, '{', '}')
            .ok_or_else(|| format!("struct option {ident} has unclosed initializer"))?;
        let inner = &source[(brace_pos + 1)..close];

        let mut options = Vec::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut in_char = false;
        let mut escape = false;
        let mut element_start: Option<usize> = None;

        for (offset, ch) in inner.char_indices() {
            if in_string {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if in_char {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '\'' {
                    in_char = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '\'' => in_char = true,
                '{' => {
                    depth += 1;
                    if depth == 1 {
                        element_start = Some(offset + 1);
                    }
                }
                '}' => {
                    if depth == 1 {
                        if let Some(start) = element_start.take() {
                            let element = inner[start..offset].trim();
                            let fields = split_top_level(element, ',');
                            if fields.len() >= 4 {
                                let name_raw = fields[0].trim();
                                if matches!(name_raw, "0" | "NULL") {
                                    break;
                                }
                                let name =
                                    parse_c_string_literal_expr(name_raw).ok_or_else(|| {
                                        "option name is not a string literal".to_string()
                                    })?;
                                if name.chars().any(char::is_whitespace) {
                                    return Err(format!("option name contains whitespace: {name}"));
                                }
                                let long_arg = parse_has_arg(&fields[1]).ok_or_else(|| {
                                    format!(
                                        "invalid has_arg value for option {name}: {}",
                                        fields[1]
                                    )
                                })?;
                                let short = parse_c_char_literal(&fields[3]);
                                options.push(GetoptLongOptionSpec {
                                    long_name: name,
                                    long_arg,
                                    short,
                                    short_arg: None,
                                });
                            }
                        }
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        return Ok(options);
    }

    Err(format!("struct option array not found: {ident}"))
}

fn extract_getopt_long_spec_from_file(path: &Path) -> Result<GetoptLongSpec, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let source = strip_c_comments(&raw);

    let (optstring, long_options_ident) =
        parse_getopt_long_call(&source).ok_or_else(|| "getopt_long call not found".to_string())?;
    let opt_map = parse_optstring(&optstring);

    let mut options = parse_struct_option_array(&source, &long_options_ident)?;
    if options.is_empty() {
        return Err("no struct option entries found".to_string());
    }

    for opt in &mut options {
        if let Some(short) = opt.short {
            opt.short_arg = opt_map.get(&short).copied();
        }
    }

    Ok(GetoptLongSpec { options })
}

fn try_extract_getopt_long_spec(passthrough: &[String]) -> (Option<GetoptLongSpec>, Vec<String>) {
    let mut notes = Vec::new();
    let candidates: Vec<PathBuf> = collect_source_candidates(passthrough);

    for path in candidates {
        match extract_getopt_long_spec_from_file(&path) {
            Ok(spec) => {
                notes.push(format!("getopt_long extracted from {}", path.display()));
                return (Some(spec), notes);
            }
            Err(err) => {
                notes.push(format!(
                    "getopt_long extractor failed for {}: {err}",
                    path.display()
                ));
            }
        }
    }

    (None, notes)
}

fn parse_argp_parse_call(source: &str) -> Option<String> {
    const NAME: &str = "argp_parse";
    for (idx, _) in source.match_indices(NAME) {
        let before_ok = idx == 0
            || !source[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric());
        if !before_ok {
            continue;
        }
        let after_idx = idx + NAME.len();
        let after_ok = after_idx == source.len()
            || !source[after_idx..]
                .chars()
                .next()
                .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric());
        if !after_ok {
            continue;
        }

        let mut cursor = after_idx;
        while let Some(ch) = source[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        if !source[cursor..].starts_with('(') {
            continue;
        }

        let close = find_matching_delimiter(source, cursor, '(', ')')?;
        let inner = &source[(cursor + 1)..close];
        let args = split_top_level(inner, ',');
        let first = args.first()?;
        let argp_ident = parse_c_identifier(first)?;
        return Some(argp_ident);
    }
    None
}

fn parse_struct_argp_options_ident(source: &str, ident: &str) -> Result<String, String> {
    const NEEDLE: &str = "struct argp";
    for (idx, _) in source.match_indices(NEEDLE) {
        let after = idx + NEEDLE.len();
        if source[after..]
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            continue;
        }
        let mut cursor = after;
        while let Some(ch) = source[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        if source[cursor..].starts_with('*') {
            cursor += 1;
            while let Some(ch) = source[cursor..].chars().next() {
                if ch.is_whitespace() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
        }

        if !source[cursor..].starts_with(ident) {
            continue;
        }
        let after_ident = cursor + ident.len();
        if source[after_ident..]
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            continue;
        }

        let eq_pos = source[after_ident..]
            .find('=')
            .map(|p| after_ident + p)
            .ok_or_else(|| format!("struct argp {ident} missing '='"))?;
        let brace_pos = source[eq_pos..]
            .find('{')
            .map(|p| eq_pos + p)
            .ok_or_else(|| format!("struct argp {ident} missing initializer"))?;

        let close = find_matching_delimiter(source, brace_pos, '{', '}')
            .ok_or_else(|| format!("struct argp {ident} has unclosed initializer"))?;
        let inner = &source[(brace_pos + 1)..close];

        let fields = split_top_level(inner, ',');
        let first = fields
            .first()
            .ok_or_else(|| format!("struct argp {ident} initializer is empty"))?;
        let options_raw = first.trim();
        if matches!(options_raw, "0" | "NULL") {
            return Err(format!("struct argp {ident} options pointer is null"));
        }

        let options_ident = parse_c_identifier(options_raw)
            .ok_or_else(|| format!("struct argp {ident} options is not an identifier"))?;
        return Ok(options_ident);
    }

    Err(format!("struct argp initializer not found: {ident}"))
}

fn contains_argp_optional_flag(expr: &str) -> bool {
    const NEEDLE: &str = "OPTION_ARG_OPTIONAL";
    for (idx, _) in expr.match_indices(NEEDLE) {
        let before_ok = idx == 0
            || !expr[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric());
        if !before_ok {
            continue;
        }
        let after_idx = idx + NEEDLE.len();
        let after_ok = after_idx == expr.len()
            || !expr[after_idx..]
                .chars()
                .next()
                .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn parse_struct_argp_option_array(
    source: &str,
    ident: &str,
) -> Result<Vec<ArgpOptionSpec>, String> {
    const NEEDLE: &str = "struct argp_option";
    for (idx, _) in source.match_indices(NEEDLE) {
        let after = idx + NEEDLE.len();
        let mut cursor = after;
        while let Some(ch) = source[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        if source[cursor..].starts_with('*') {
            cursor += 1;
            while let Some(ch) = source[cursor..].chars().next() {
                if ch.is_whitespace() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
        }

        if !source[cursor..].starts_with(ident) {
            continue;
        }
        let after_ident = cursor + ident.len();
        if source[after_ident..]
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            continue;
        }

        let eq_pos = source[after_ident..]
            .find('=')
            .map(|p| after_ident + p)
            .ok_or_else(|| format!("struct argp_option {ident} missing '='"))?;
        let brace_pos = source[eq_pos..]
            .find('{')
            .map(|p| eq_pos + p)
            .ok_or_else(|| format!("struct argp_option {ident} missing initializer"))?;

        let close = find_matching_delimiter(source, brace_pos, '{', '}')
            .ok_or_else(|| format!("struct argp_option {ident} has unclosed initializer"))?;
        let inner = &source[(brace_pos + 1)..close];

        let mut options = Vec::new();
        let mut depth = 0usize;
        let mut in_string = false;
        let mut in_char = false;
        let mut escape = false;
        let mut element_start: Option<usize> = None;

        for (offset, ch) in inner.char_indices() {
            if in_string {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if in_char {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    continue;
                }
                if ch == '\'' {
                    in_char = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '\'' => in_char = true,
                '{' => {
                    depth += 1;
                    if depth == 1 {
                        element_start = Some(offset + 1);
                    }
                }
                '}' => {
                    if depth == 1 {
                        if let Some(start) = element_start.take() {
                            let element = inner[start..offset].trim();
                            let fields = split_top_level(element, ',');

                            let name_raw = fields.first().map(String::as_str).unwrap_or("0").trim();
                            let key_raw = fields.get(1).map(String::as_str).unwrap_or("0").trim();
                            let arg_raw = fields.get(2).map(String::as_str).unwrap_or("0").trim();
                            let flags_raw = fields.get(3).map(String::as_str).unwrap_or("0").trim();
                            let doc_raw = fields.get(4).map(String::as_str).unwrap_or("0").trim();
                            let group_raw = fields.get(5).map(String::as_str).unwrap_or("0").trim();

                            let terminator = matches!(name_raw, "0" | "NULL")
                                && matches!(key_raw, "0" | "NULL")
                                && matches!(arg_raw, "0" | "NULL")
                                && matches!(flags_raw, "0" | "NULL")
                                && matches!(doc_raw, "0" | "NULL")
                                && matches!(group_raw, "0" | "NULL");
                            if terminator {
                                break;
                            }

                            if matches!(name_raw, "0" | "NULL") {
                                continue;
                            }

                            let long_name =
                                parse_c_string_literal_expr(name_raw).ok_or_else(|| {
                                    "argp option name is not a string literal".to_string()
                                })?;
                            if long_name.chars().any(char::is_whitespace) {
                                return Err(format!(
                                    "argp option name contains whitespace: {long_name}"
                                ));
                            }

                            let short = match key_raw {
                                "0" | "NULL" => None,
                                _ => Some(parse_c_char_literal(key_raw).ok_or_else(|| {
                                    format!("argp option key for {long_name} is not a char literal")
                                })?),
                            };

                            let has_arg = match arg_raw {
                                "0" | "NULL" => false,
                                _ => parse_c_string_literal_expr(arg_raw)
                                    .ok_or_else(|| {
                                        format!("argp option arg for {long_name} is not a string literal")
                                    })
                                    .map(|_| true)?,
                            };
                            let optional_arg = contains_argp_optional_flag(flags_raw);
                            let arg_requirement = if optional_arg {
                                OptionArgRequirement::Optional
                            } else if has_arg {
                                OptionArgRequirement::Required
                            } else {
                                OptionArgRequirement::None
                            };

                            let doc = match doc_raw {
                                "0" | "NULL" => None,
                                _ => Some(parse_c_string_literal_expr(doc_raw).ok_or_else(|| {
                                    format!("argp option doc for {long_name} is not a string literal")
                                })?),
                            };

                            options.push(ArgpOptionSpec {
                                long_name,
                                arg_requirement,
                                short,
                                doc,
                            });
                        }
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        return Ok(options);
    }

    Err(format!("struct argp_option array not found: {ident}"))
}

fn parse_any_struct_argp_option_array(
    source: &str,
) -> Result<(String, Vec<ArgpOptionSpec>), String> {
    const NEEDLE: &str = "struct argp_option";
    for (idx, _) in source.match_indices(NEEDLE) {
        let after = idx + NEEDLE.len();
        let mut cursor = after;
        while let Some(ch) = source[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        if source[cursor..].starts_with('*') {
            cursor += 1;
            while let Some(ch) = source[cursor..].chars().next() {
                if ch.is_whitespace() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
        }

        let ident = match parse_c_identifier(&source[cursor..]) {
            Some(v) => v,
            None => continue,
        };
        match parse_struct_argp_option_array(source, &ident) {
            Ok(options) if !options.is_empty() => return Ok((ident, options)),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }

    Err("struct argp_option array not found".to_string())
}

fn extract_argp_spec_from_file(path: &Path) -> Result<ArgpSpec, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let source = strip_c_comments(&raw);

    if let Some(argp_ident) = parse_argp_parse_call(&source) {
        if let Ok(options_ident) = parse_struct_argp_options_ident(&source, &argp_ident) {
            if let Ok(options) = parse_struct_argp_option_array(&source, &options_ident) {
                if !options.is_empty() {
                    return Ok(ArgpSpec { options });
                }
            }
        }
    }

    let (_ident, options) = parse_any_struct_argp_option_array(&source)?;
    if options.is_empty() {
        return Err("no struct argp_option entries found".to_string());
    }
    Ok(ArgpSpec { options })
}

fn collect_source_candidates(passthrough: &[String]) -> Vec<PathBuf> {
    // In simple invocations, the link command contains .c/.cpp files directly.
    // In CMake multi-file projects, the final link step usually contains only object files,
    // commonly named like: CMakeFiles/<tgt>.dir/src/main.c.o
    // We can often recover the original source path by stripping the trailing `.o`.
    let mut out = Vec::new();

    for arg in passthrough.iter().filter(|arg| !arg.starts_with('-')) {
        let p = PathBuf::from(arg);

        // Direct source.
        if p.exists() {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default();
            if matches!(ext, "c" | "cc" | "cpp" | "cxx" | "C") {
                out.push(p);
                continue;
            }
        }

        // Object file with embedded source extension: `something.c.o` → try to recover `something.c`.
        // CMake often produces: CMakeFiles/<tgt>.dir/<relpath>.c.o
        // where <relpath>.c is relative to the *source* dir, not the build dir.
        if let Some(s) = p.to_str() {
            if s.ends_with(".o") {
                let stem = &s[..s.len() - 2];

                // 1) Direct sibling (works for some build systems).
                let direct = PathBuf::from(stem);
                if direct.exists() {
                    let ext = direct.extension().and_then(|e| e.to_str()).unwrap_or_default();
                    if matches!(ext, "c" | "cc" | "cpp" | "cxx" | "C") {
                        out.push(direct);
                        continue;
                    }
                }

                // 2) CMake heuristic: take the portion after `.dir/` and search upwards.
                if let Some(pos) = stem.find(".dir/") {
                    let rel = &stem[(pos + 5)..]; // after `.dir/`
                    // rel likely ends with `.c` / `.cpp` etc.
                    let mut base = PathBuf::from("..");
                    for _ in 0..4 {
                        let cand = base.join(rel);
                        if cand.exists() {
                            let ext = cand.extension().and_then(|e| e.to_str()).unwrap_or_default();
                            if matches!(ext, "c" | "cc" | "cpp" | "cxx" | "C") {
                                out.push(cand);
                                break;
                            }
                        }
                        base = base.join("..");
                    }
                }
            }
        }
    }

    // Preserve determinism.
    out.sort();
    out.dedup();
    out
}

fn try_extract_argp_spec(passthrough: &[String]) -> (Option<ArgpSpec>, Vec<String>) {
    let mut notes = Vec::new();
    let candidates: Vec<PathBuf> = collect_source_candidates(passthrough);

    for path in candidates {
        match extract_argp_spec_from_file(&path) {
            Ok(spec) => {
                notes.push(format!("argp extracted from {}", path.display()));
                return (Some(spec), notes);
            }
            Err(err) => {
                notes.push(format!(
                    "argp extractor failed for {}: {err}",
                    path.display()
                ));
            }
        }
    }

    (None, notes)
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

#[cfg(windows)]
const PACKAGED_SERVER_BINARY_NAMES: [&str; 2] = ["mcpcc-mcp-server.exe", "mcpcc-mcp-server"];
#[cfg(not(windows))]
const PACKAGED_SERVER_BINARY_NAMES: [&str; 1] = ["mcpcc-mcp-server"];

fn resolve_packaged_mcp_server_binary() -> Result<PathBuf, ServerCopyError> {
    let mut searched = Vec::new();
    let current_exe = std::env::current_exe().map_err(|err| ServerCopyError::CurrentExe(err))?;
    if let Some(dir) = current_exe.parent() {
        for name in PACKAGED_SERVER_BINARY_NAMES {
            let candidate = dir.join(name);
            searched.push(candidate.clone());
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }

    if let Some(path_env) = std::env::var_os("PATH") {
        for name in PACKAGED_SERVER_BINARY_NAMES {
            if let Some(path) = find_executable(name, Some(path_env.as_os_str())) {
                return Ok(path);
            }
        }
    }

    Err(ServerCopyError::NotFound(searched))
}

pub fn copy_packaged_mcp_server_binary(out_path: &Path) -> Result<(), ServerCopyError> {
    let src = resolve_packaged_mcp_server_binary()?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(ServerCopyError::Io)?;
        }
    }

    std::fs::copy(src, out_path).map_err(ServerCopyError::Io)?;
    Ok(())
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
