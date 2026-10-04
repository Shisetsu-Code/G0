//! Export the canonical G0 compiler source or execute its backend.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    match args.as_slice() {
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
                "usage: bootstrap_compiler source output.g0p | compile compiler.g0p input.g0p"
                    .into(),
            );
        }
    }
    Ok(())
}
