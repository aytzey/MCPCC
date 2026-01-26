use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use object::{Object, ObjectSection};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionArgRequirement {
    None,
    Required,
    Optional,
}

impl OptionArgRequirement {
    fn as_str(self) -> &'static str {
        match self {
            OptionArgRequirement::None => "none",
            OptionArgRequirement::Required => "required",
            OptionArgRequirement::Optional => "optional",
        }
    }
}

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

const MCPCC_ANNOT_SECTION: &str = ".mcpcc";
const MCPCC_TOOL_PREFIX: &str = "MCPCC_TOOL:";
const MCPCC_PARAM_PREFIX: &str = "MCPCC_PARAM:";

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

fn build_run_raw_tool_json(
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
    })
}

fn build_getopt_long_structured_tool_json(
    artifacts: &ArtifactPaths,
    spec: &GetoptLongSpec,
) -> serde_json::Value {
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
    for opt in &spec.options {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "param".to_string(),
            serde_json::Value::String(opt.long_name.clone()),
        );
        entry.insert(
            "long".to_string(),
            serde_json::Value::String(format!("--{}", opt.long_name)),
        );
        entry.insert(
            "arg".to_string(),
            serde_json::Value::String(opt.long_arg.as_str().to_string()),
        );
        if let Some(short) = opt.short {
            entry.insert(
                "short".to_string(),
                serde_json::Value::String(format!("-{short}")),
            );
        }
        if let Some(short_arg) = opt.short_arg {
            entry.insert(
                "shortArg".to_string(),
                serde_json::Value::String(short_arg.as_str().to_string()),
            );
        }
        mapping_options.push(serde_json::Value::Object(entry));
    }

    serde_json::json!({
        "name": artifacts.base_name,
        "description": format!("Run {} with structured options.", artifacts.base_name),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        },
        "x-mcpcc": {
            "argvMapping": {
                "options": mapping_options,
                "argsParam": "args",
            },
        },
    })
}

fn build_argp_structured_tool_json(
    artifacts: &ArtifactPaths,
    spec: &ArgpSpec,
) -> serde_json::Value {
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
    for opt in &spec.options {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "param".to_string(),
            serde_json::Value::String(opt.long_name.clone()),
        );
        entry.insert(
            "long".to_string(),
            serde_json::Value::String(format!("--{}", opt.long_name)),
        );
        entry.insert(
            "arg".to_string(),
            serde_json::Value::String(opt.arg_requirement.as_str().to_string()),
        );
        if let Some(short) = opt.short {
            entry.insert(
                "short".to_string(),
                serde_json::Value::String(format!("-{short}")),
            );
            entry.insert(
                "shortArg".to_string(),
                serde_json::Value::String(opt.arg_requirement.as_str().to_string()),
            );
        }
        mapping_options.push(serde_json::Value::Object(entry));
    }

    serde_json::json!({
        "name": artifacts.base_name,
        "description": format!("Run {} with structured options.", artifacts.base_name),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        },
        "x-mcpcc": {
            "argvMapping": {
                "options": mapping_options,
                "argsParam": "args",
            },
        },
    })
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

    serde_json::json!({
        "mcpccVersion": env!("CARGO_PKG_VERSION"),
        "mcpSpecVersion": MCP_SPEC_VERSION,
        "binary": {
            "path": artifacts.bin_path.to_string_lossy(),
        },
        "tools": tools,
    })
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

    let name = obj
        .get("name")
        .and_then(non_empty_string)
        .ok_or_else(|| "tool annotation missing required string field: name".to_string())?;

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

    let tool = obj
        .get("tool")
        .and_then(non_empty_string)
        .ok_or_else(|| "param annotation missing required string field: tool".to_string())?;
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

fn annotation_arg_requirement(annotation: &ParamAnnotation) -> Option<&'static str> {
    if annotation.ty.is_none() && annotation.takes_value.is_none() {
        return None;
    }

    match annotation_schema_type(annotation) {
        Some("boolean") => Some("none"),
        Some(_) => Some("required"),
        None => None,
    }
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
        if opt_map.get("param").and_then(|v| v.as_str()) == Some(annotation.property.as_str()) {
            option_obj = Some(opt_map);
            break;
        }
    }

    if option_obj.is_none() {
        let mut new = serde_json::Map::new();
        new.insert(
            "param".to_string(),
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
        if let Some(arg) = annotation_arg_requirement(annotation) {
            new.insert(
                "arg".to_string(),
                serde_json::Value::String(arg.to_string()),
            );
            if new.get("short").is_some() {
                new.insert(
                    "shortArg".to_string(),
                    serde_json::Value::String(arg.to_string()),
                );
            }
        }
        if let Some(repeatable) = annotation.repeatable {
            new.insert(
                "repeatable".to_string(),
                serde_json::Value::Bool(repeatable),
            );
        }
        options.push(serde_json::Value::Object(new));
        return true;
    }

    let opt_map = option_obj.expect("option object");
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

    if let Some(arg) = annotation_arg_requirement(annotation) {
        opt_map.insert(
            "arg".to_string(),
            serde_json::Value::String(arg.to_string()),
        );
        if opt_map.get("short").is_some() {
            opt_map.insert(
                "shortArg".to_string(),
                serde_json::Value::String(arg.to_string()),
            );
        }
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
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    let mut mapping_options = Vec::new();
    for annotation in params {
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
            "param".to_string(),
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
        let arg = annotation_arg_requirement(annotation).unwrap_or_else(|| {
            if schema_type == "boolean" {
                "none"
            } else {
                "required"
            }
        });
        entry.insert(
            "arg".to_string(),
            serde_json::Value::String(arg.to_string()),
        );
        if entry.get("short").is_some() {
            entry.insert(
                "shortArg".to_string(),
                serde_json::Value::String(arg.to_string()),
            );
        }
        if let Some(repeatable) = annotation.repeatable {
            entry.insert(
                "repeatable".to_string(),
                serde_json::Value::Bool(repeatable),
            );
        }
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

    serde_json::json!({
        "name": artifacts.base_name,
        "description": format!("Run {} with structured options.", artifacts.base_name),
        "inputSchema": serde_json::Value::Object(input_schema),
        "x-mcpcc": {
            "argvMapping": {
                "options": mapping_options,
                "argsParam": "args",
            },
        },
    })
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
    descriptions: &LlmToolDescriptions,
    passthrough: &[String],
) -> Result<AnalysisSummary, JsonWriteError> {
    let (getopt_long_spec, mut notes) = try_extract_getopt_long_spec(passthrough);
    let (argp_spec, argp_notes) = if getopt_long_spec.is_some() {
        (None, Vec::new())
    } else {
        try_extract_argp_spec(passthrough)
    };
    notes.extend(argp_notes);

    let annotations = read_mcpcc_annotations(&artifacts.bin_path);
    notes.extend(annotations.notes.iter().cloned());

    let mut analysis = AnalysisSummary::default();
    analysis.notes = notes;

    let mut structured_tool = if let Some(spec) = getopt_long_spec.as_ref() {
        analysis.extractors = vec!["getopt_long".to_string()];
        analysis.structured_tool_generated = true;
        analysis.param_count = spec.options.len() + 1;
        Some(build_getopt_long_structured_tool_json(artifacts, spec))
    } else if let Some(spec) = argp_spec.as_ref() {
        analysis.extractors = vec!["argp".to_string()];
        analysis.structured_tool_generated = true;
        analysis.param_count = spec.options.len() + 1;
        Some(build_argp_structured_tool_json(artifacts, spec))
    } else {
        None
    };

    if structured_tool.is_none() {
        let param_annotations: Vec<ParamAnnotation> = annotations
            .params
            .iter()
            .filter(|a| a.tool == artifacts.base_name)
            .cloned()
            .collect();
        if !param_annotations.is_empty() {
            structured_tool = Some(build_annotation_structured_tool_json(
                artifacts,
                &param_annotations,
            ));
            analysis.structured_tool_generated = true;
            analysis.extractors = vec!["annotation".to_string()];
        }
    }

    let mut mcp_json = build_mcp_json(artifacts, descriptions, structured_tool);
    let used_annotations = apply_annotations_to_mcp_json(&mut mcp_json, &annotations);
    if used_annotations && !analysis.extractors.iter().any(|e| e == "annotation") {
        analysis.extractors.insert(0, "annotation".to_string());
    }
    if analysis.structured_tool_generated {
        if let Some(param_count) = count_tool_param_count(&mcp_json, &artifacts.base_name) {
            analysis.param_count = param_count;
        }
    }

    write_json_atomic(&artifacts.mcp_json_path, &mcp_json)?;

    Ok(analysis)
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
    let candidates: Vec<PathBuf> = passthrough
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .filter(|path| {
            if !path.exists() {
                return false;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            matches!(ext, "c" | "cc" | "cpp" | "cxx" | "C")
        })
        .collect();

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

fn try_extract_argp_spec(passthrough: &[String]) -> (Option<ArgpSpec>, Vec<String>) {
    let mut notes = Vec::new();
    let candidates: Vec<PathBuf> = passthrough
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .filter(|path| {
            if !path.exists() {
                return false;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            matches!(ext, "c" | "cc" | "cpp" | "cxx" | "C")
        })
        .collect();

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
