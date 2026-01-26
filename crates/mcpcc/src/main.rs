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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let parsed = match mcpcc::parse_args(&argv) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("mcpcc: {err}");
            return ExitCode::from(2);
        }
    };

    let llm_env = mcpcc::LlmEnv::from_current();
    if parsed.wrapper.llm_mode == mcpcc::LlmMode::Off && !llm_env.allow_no_llm {
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

    let artifacts = mcpcc::plan_artifacts(&parsed.wrapper, &parsed.passthrough);

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
        let (descriptions, llm_manifest) =
            match mcpcc::generate_run_raw_llm_descriptions(&artifacts, &parsed.wrapper, &llm_env) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("mcpcc: failed to generate LLM descriptions: {err}");
                    return ExitCode::from(70);
                }
            };

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

        let analysis =
            match mcpcc::write_mcp_json_atomic(&artifacts, &descriptions, &parsed.passthrough) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("mcpcc: failed to write mcp.json: {err}");
                    return ExitCode::from(70);
                }
            };
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
