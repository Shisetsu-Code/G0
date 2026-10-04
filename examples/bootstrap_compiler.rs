//! Export the canonical G0 compiler source or execute its backend.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    match args.as_slice() {
        [_, command] if command == "profile" => {
            let result = std::thread::Builder::new()
                .stack_size(g0::bootstrap_compiler::HOST_STACK_BYTES)
                .spawn(|| {
                    let document = g0::bootstrap_compiler::compiler_document();
                    let source = g0::program_binary::encode_program(&document).unwrap();
                    let contract = document.validated_contract().unwrap();
                    let mut limits = g0::bootstrap_compiler::compiler_limits();
                        limits.max_value_bytes = 64 * 1024 * 1024 * 1024;
                        limits.max_steps = 64_000_000;
                    let mut executor = g0::execution::Executor::new(&contract, limits).unwrap();
                    let result = executor.run_graph(
                        "compile",
                        vec![g0::value::Value::Bytes(source.clone().into())],
                    );
                    println!(
                        "source_bytes={} steps={} logical_bytes={} result={}",
                        source.len(),
                        executor.steps_used(),
                        executor.value_bytes_used(),
                        match result {
                            Ok(_) => "ok".into(),
                            Err(error) => format!("{error:?}"),
                        }
                    );
                })?
                .join();
            if result.is_err() {
                return Err("compiler profiling worker failed".into());
            }
        }
        [_, command, source] if command == "direct" => print!(
            "{}",
            g0::bootstrap_compiler::compile_direct_native(&std::fs::read(source)?)?
        ),
        [_, command, path] if command == "source" => std::fs::write(
            path,
            g0::program_binary::encode_program(&g0::bootstrap_compiler::compiler_document())
                .map_err(|e| format!("{e:?}"))?,
        )?,
        [_, command, compiler, source] if command == "compile" => print!(
            "{}",
            g0::bootstrap_compiler::compile_with(
                &std::fs::read(compiler)?,
                &std::fs::read(source)?
            )?
        ),
        _ => {
            return Err(
                "usage: bootstrap_compiler source output.g0p | compile compiler.g0p input.g0p | direct input.g0p | profile"
                    .into(),
            );
        }
    }
    Ok(())
}
