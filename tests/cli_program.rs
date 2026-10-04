use g0::program_binary::encode_program;
use std::path::PathBuf;
use std::process::{Command, Output};
#[path = "support/program_fixture.rs"]
mod fixture;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "g0-program-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_g0c"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_program_cli_compiles_calls_select_and_loop() {
    let work = Workspace::new();
    for document in [
        fixture::call_program(),
        fixture::select_program(),
        fixture::loop_program(),
    ] {
        std::fs::write(
            work.0.join("program.g0p"),
            encode_program(&document).unwrap(),
        )
        .unwrap();
        success(work.run(&["check", "program.g0p"]));
        success(work.run(&["compile", "program.g0p", "-o", "program.s"]));
        assert!(
            std::fs::read_to_string(work.0.join("program.s"))
                .unwrap()
                .contains("g0_machine_main:")
        );
    }
}

#[test]
fn native_program_magic_does_not_require_a_specific_extension() {
    let work = Workspace::new();
    std::fs::write(
        work.0.join("program.data"),
        encode_program(&fixture::call_program()).unwrap(),
    )
    .unwrap();
    success(work.run(&["compile", "program.data"]));
    assert!(work.0.join("program.s").is_file());
}

#[test]
fn malformed_programs_preserve_output_and_do_not_run_as_text() {
    let work = Workspace::new();
    for bytes in [b"G0P\0".as_slice(), b"g0 0.1".as_slice()] {
        std::fs::write(work.0.join("bad.g0p"), bytes).unwrap();
        std::fs::write(work.0.join("result.s"), "preserve me").unwrap();
        let output = work.run(&["compile", "bad.g0p", "-o", "result.s"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("native program"));
        assert_eq!(
            std::fs::read_to_string(work.0.join("result.s")).unwrap(),
            "preserve me"
        );
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn program_files_execute_calls_selection_and_bounded_iteration() {
    let work = Workspace::new();
    for (name, expected) in [("call", 42), ("select", 42), ("loop", 0)] {
        let example =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("examples/{name}.g0p"));
        std::fs::copy(example, work.0.join("program.g0p")).unwrap();
        success(work.run(&["compile", "program.g0p"]));
        std::fs::write(work.0.join("harness.c"), format!("long g0_machine_main(void);\nint main(void) {{ return g0_machine_main() == {expected} ? 0 : 1; }}\n")).unwrap();
        success(
            Command::new("cc")
                .current_dir(&work.0)
                .args(["program.s", "harness.c", "-o", "program"])
                .output()
                .unwrap(),
        );
        success(Command::new(work.0.join("program")).output().unwrap());
    }
}
