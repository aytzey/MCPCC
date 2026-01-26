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

    if let Some(artifacts) = mcpcc::plan_artifacts(&parsed.wrapper, &parsed.passthrough) {
        if let Err(err) = mcpcc::write_mcp_json_atomic(&artifacts) {
            eprintln!("mcpcc: failed to write mcp.json: {err}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}
