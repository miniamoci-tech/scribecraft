//! The ScribeCraft desktop app.

// No console window behind the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    let files: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    match scribecraft_ui_egui::run(files) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ScribeCraft could not start: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
