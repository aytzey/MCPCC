use std::process::ExitCode;

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

    eprintln!("mcpcc: passthrough compilation not implemented yet");
    ExitCode::from(2)
}
