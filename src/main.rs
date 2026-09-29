use std::{env, fs, path::PathBuf, process::ExitCode};

fn usage() {
    eprintln!("g0c 0.1.0\n\nUSAGE:\n    g0c check <input.g0>\n    g0c compile <input.g0> [-o output.s]");
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("g0c: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        usage();
        return Err("missing command or input".into());
    }

    let command = &args[1];
    let input = PathBuf::from(&args[2]);
    let source = fs::read_to_string(&input)?;

    match command.as_str() {
        "check" => {
            let graph = g0::parser::parse(&source)?;
            g0::validate::validate(&graph)?;
            println!("ok: {} nodes, target {}", graph.nodes.len(), graph.target);
        }
        "compile" => {
            let asm = g0::compile_source(&source)?;
            let output = if args.len() >= 5 && args[3] == "-o" {
                PathBuf::from(&args[4])
            } else {
                input.with_extension("s")
            };
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, asm)?;
            println!("{}", output.display());
        }
        _ => {
            usage();
            return Err(format!("unknown command '{command}'").into());
        }
    }
    Ok(())
}
