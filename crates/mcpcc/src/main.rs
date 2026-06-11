use std::process::ExitCode;

fn exit_code_from_status(status: std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        if let Ok(code) = u8::try_from(code) {
            return ExitCode::from(code);
        }
        return ExitCode::from(1);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            if let Ok(code) = u8::try_from(128 + signal) {
                return ExitCode::from(code);
            }
        }
    }

    ExitCode::from(1)
}

const USAGE: &str = "\
mcpcc - C/C++ compiler wrapper that emits MCP tool artifacts for linked executables

USAGE:
    mcpcc [MCPCC_FLAGS...] -- [COMPILER_ARGS...]
    mcpcc [MCPCC_FLAGS_AND_COMPILER_ARGS...]   (flags prefixed --mcpcc- are consumed)

MCPCC FLAGS:
    --mcpcc-cc <path>             Underlying compiler (else $MCPCC_CC, $CC, clang, gcc)
    --mcpcc-print-cc              Print the resolved compiler path and exit
    --mcpcc-artifacts-dir <dir>   Artifact output dir (default: binary's directory)
    --mcpcc-mcp-json-out <path>   Override mcp.json output path
    --mcpcc-server-out <path>     Override MCP server binary output path
    --mcpcc-manifest-out <path>   Override manifest output path
    --mcpcc-llm-mode <mode>       required|best-effort|off (default: required;
                                  off needs MCPCC_ALLOW_NO_LLM=1)
    --mcpcc-llm-model <id>        OpenRouter model id
    --mcpcc-cache-dir <dir>       LLM cache dir (default: ~/.cache/mcpcc)
    --mcpcc-verbose               Verbose diagnostics on stderr
    --mcpcc-version               Print version and exit
    --mcpcc-help                  Print this help and exit

ENVIRONMENT:
    OPENROUTER_API_KEY            Required in llm-mode=required
    MCPCC_CC, MCPCC_LLM_MODE, MCPCC_LLM_MODEL, MCPCC_CACHE_DIR,
    MCPCC_ARTIFACTS_DIR, MCPCC_ALLOW_NO_LLM, MCPCC_OPENROUTER_BASE_URL

On a successful executable link, mcpcc writes <bin>.mcp.json, <bin>.mcp-server,
and <bin>.mcpcc-manifest.json next to the binary.";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let mut parsed = match mcpcc::parse_args(&argv) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("mcpcc: {err}");
            return ExitCode::from(2);
        }
    };

    if parsed.wrapper.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    if parsed.wrapper.version {
        println!("mcpcc {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let llm_mode = match mcpcc::resolve_llm_mode(&parsed.wrapper) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("mcpcc: {msg}");
            return ExitCode::from(2);
        }
    };
    parsed.wrapper.llm_mode = Some(llm_mode);

    let llm_env = mcpcc::LlmEnv::from_current();
    if llm_mode == mcpcc::LlmMode::Off && !llm_env.allow_no_llm {
        eprintln!("mcpcc: --mcpcc-llm-mode=off requires MCPCC_ALLOW_NO_LLM=1");
        return ExitCode::from(2);
    }

    let env = mcpcc::EnvSnapshot::from_current();
    let compiler = match mcpcc::resolve_underlying_compiler(&parsed.wrapper, &env) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("mcpcc: {err}");
            return ExitCode::from(2);
        }
    };

    if parsed.wrapper.print_cc {
        println!("{}", compiler.display());
        return ExitCode::SUCCESS;
    }

    if parsed.wrapper.verbose {
        eprintln!("mcpcc: using compiler: {}", compiler.display());
    }

    let status = match std::process::Command::new(&compiler)
        .args(&parsed.passthrough)
        .status()
    {
        Ok(v) => v,
        Err(err) => {
            eprintln!("mcpcc: failed to exec {}: {err}", compiler.display());
            return ExitCode::from(2);
        }
    };

    if !status.success() {
        return exit_code_from_status(status);
    }

    // Analysis works on the expanded argv so `@response-file` link lines
    // (e.g. CMake+Ninja) still reveal `-o` and the source/object files.
    let analysis_args = mcpcc::expand_response_files(&parsed.passthrough);

    // On `-c` compile steps, remember which sources produced which objects so
    // the later link step can run the extractors on them (autoconf/make trees).
    mcpcc::record_compile_step_sources(&analysis_args);

    let artifacts = mcpcc::plan_artifacts(&parsed.wrapper, &analysis_args);

    if parsed.wrapper.verbose {
        match &artifacts {
            Some(plan) => {
                eprintln!("mcpcc: link detected; generating MCP artifacts");
                eprintln!("mcpcc: bin_path: {}", plan.bin_path.display());
                eprintln!("mcpcc: mcp_json: {}", plan.mcp_json_path.display());
                eprintln!("mcpcc: server: {}", plan.server_path.display());
                eprintln!("mcpcc: manifest: {}", plan.manifest_path.display());
            }
            None => eprintln!("mcpcc: compile-only mode detected; skipping MCP artifacts"),
        }
    }

    if let Some(artifacts) = artifacts {
        let mut plan = match mcpcc::plan_mcp_json(&artifacts, &analysis_args) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("mcpcc: failed to plan mcp.json: {err}");
                return ExitCode::from(70);
            }
        };

        let (descriptions, llm_manifest) = match mcpcc::generate_llm_descriptions(
            &parsed.wrapper,
            &llm_env,
            &plan.llm_summary_json,
            &plan.llm_expected,
        ) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("mcpcc: failed to generate LLM descriptions: {err}");
                return ExitCode::from(70);
            }
        };

        plan.apply_llm_descriptions(&descriptions);

        if parsed.wrapper.verbose {
            eprintln!(
                "mcpcc: llm: mode={} provider={} model={} cacheHit={} usedPlaceholder={}",
                llm_manifest.mode,
                llm_manifest.provider,
                llm_manifest.model,
                llm_manifest.cache_hit,
                llm_manifest.used_placeholder
            );
            if let Some(err) = llm_manifest.error.as_deref() {
                eprintln!("mcpcc: llm: note: {err}");
            }
        }

        let analysis = plan.analysis.clone();
        if let Err(err) = mcpcc::write_mcp_json_atomic(&artifacts, &plan) {
            eprintln!("mcpcc: failed to write mcp.json: {err}");
            return ExitCode::from(70);
        }
        if parsed.wrapper.verbose {
            if analysis.structured_tool_generated && !analysis.extractors.is_empty() {
                eprintln!(
                    "mcpcc: structured tool extracted via: {}",
                    analysis.extractors.join(",")
                );
            }
            if !analysis.notes.is_empty() {
                for note in &analysis.notes {
                    eprintln!("mcpcc: analysis: {note}");
                }
            }
            eprintln!(
                "mcpcc: wrote mcp.json: {}",
                artifacts.mcp_json_path.display()
            );
        }

        if let Err(err) = mcpcc::copy_packaged_mcp_server_binary(&artifacts.server_path) {
            eprintln!("mcpcc: failed to copy server: {err}");
            return ExitCode::from(70);
        }
        if parsed.wrapper.verbose {
            eprintln!("mcpcc: wrote server: {}", artifacts.server_path.display());
        }

        if let Err(err) = mcpcc::write_manifest_json_atomic(
            &compiler,
            &parsed.passthrough,
            0,
            &artifacts,
            &analysis,
            &llm_manifest,
        ) {
            eprintln!("mcpcc: failed to write manifest: {err}");
            return ExitCode::from(70);
        }
        if parsed.wrapper.verbose {
            eprintln!(
                "mcpcc: wrote manifest: {}",
                artifacts.manifest_path.display()
            );
        }
    }

    ExitCode::SUCCESS
}
