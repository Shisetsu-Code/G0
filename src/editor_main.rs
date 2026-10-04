#[cfg(windows)]
#[path = "editor_windows.rs"]
mod windows;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--help") {
        println!(
            "g0-editor [program.g0p|graph.g0g]\nNative Windows graph editor; editing never grants runtime authority."
        );
        return std::process::ExitCode::SUCCESS;
    }
    #[cfg(windows)]
    match windows::run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("g0-editor: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("g0-editor: the graphical bootstrap profile currently requires Windows");
        std::process::ExitCode::FAILURE
    }
}
