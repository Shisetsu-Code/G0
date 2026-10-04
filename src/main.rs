use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

fn usage() {
    eprintln!(
        "g0c 0.1.0\n\nUSAGE:\n    g0c check <input.g0p|input.g0g|input.g0>\n    g0c compile <input.g0p|input.g0g|input.g0> [-o output.s]\n\n.g0p: native executable program; .g0g: native canonical graph; .g0: bootstrap Graph Assembly"
    );
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
    let (command, input, output) = match args.as_slice() {
        [_, command, input] if matches!(command.as_str(), "check" | "compile") => {
            (command.as_str(), PathBuf::from(input), None)
        }
        [_, command, input, flag, output] if command == "compile" && flag == "-o" => (
            command.as_str(),
            PathBuf::from(input),
            Some(PathBuf::from(output)),
        ),
        _ => {
            usage();
            return Err("invalid command or arguments".into());
        }
    };
    let bytes = fs::read(&input)?;
    let native_program = bytes.starts_with(b"G0P\0")
        || (!bytes.starts_with(b"G0G\0")
            && input
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("g0p")));
    let native = bytes.starts_with(b"G0G\0")
        || input
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("g0g"));

    let assembly = if native_program {
        let document = g0::program_binary::decode_program(&bytes)
            .map_err(|issue| format!("native program decode failed: {issue:?}"))?;
        let program = document
            .validated_contract()
            .map_err(|issue| format!("native program validation failed: {issue:?}"))?;
        if command == "check" {
            println!(
                "ok: {} graphs, entry {}, target x86_64-v3",
                document.graphs.len(),
                document.entry_graph
            );
            return Ok(());
        }
        g0::native_program::compile_program(
            &program,
            &g0::program::PlatformContract::bootstrap_x86_64_v3(),
            g0::machine::MachineProfile::x86_64_v3(),
        )
        .map_err(|issue| format!("native program compilation failed: {issue:?}"))?
        .assembly
    } else if native {
        let graph = g0::graph_binary_decode::decode_graph(&bytes)
            .map_err(|issue| format!("native graph decode failed: {issue:?}"))?;
        let program = g0::program::ProgramContract {
            entry_graph: Some(graph.name.clone()),
            graphs: vec![graph.clone()],
            ..g0::program::ProgramContract::default()
        };
        g0::program::validate_program(
            &program,
            &g0::program::PlatformContract::bootstrap_x86_64_v3(),
        )
        .map_err(|issues| format!("native graph validation failed: {issues:?}"))?;
        if command == "check" {
            println!(
                "ok: {} nodes, native graph {}, target x86_64-v3",
                graph.nodes.len(),
                graph.name
            );
            return Ok(());
        }
        g0::compiler::compile_graph(&graph, g0::machine::MachineProfile::x86_64_v3())
            .map_err(|issue| format!("native graph compilation failed: {issue:?}"))?
            .assembly
    } else {
        let source = std::str::from_utf8(&bytes)?;
        if command == "check" {
            let graph = g0::parser::parse(source)?;
            g0::validate::validate(&graph)?;
            println!("ok: {} nodes, target {}", graph.nodes.len(), graph.target);
            return Ok(());
        }
        g0::compile_source(source)?
    };
    let output = output.unwrap_or_else(|| input.with_extension("s"));
    if fs::canonicalize(&output).ok().as_ref() == Some(&fs::canonicalize(&input)?) {
        return Err("output must not overwrite the input graph".into());
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    write_assembly(&output, &assembly)?;
    println!("{}", output.display());
    Ok(())
}

fn write_assembly(output: &Path, assembly: &str) -> std::io::Result<()> {
    // Replacing the directory entry instead of truncating the destination
    // preserves an input file that happens to share its inode with the output.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = output.with_file_name(format!(".g0c-{}-{nonce}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(assembly.as_bytes())?;
        drop(file);
        fs::rename(&temporary, output)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
