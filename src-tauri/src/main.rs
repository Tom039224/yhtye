// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use yhtye_lib::cli::{self, Invocation};

/// Looks at the command line before anything else starts: `--headless` and
/// `yhtye remote …` must run where there is no display (GTK is only
/// initialised once the window is built), and the app must say so instead of
/// panicking when there is none.
fn main() -> ExitCode {
    let invocation = match cli::parse(std::env::args_os().skip(1)) {
        Ok(invocation) => invocation,
        Err(e) => {
            eprintln!("yhtye: {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match invocation {
        Invocation::Help => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Invocation::Headless(opts) => yhtye_lib::headless::run(opts),
        Invocation::Remote { action, data_dir } => {
            yhtye_lib::headless::run_remote(&action, data_dir)
        }
        Invocation::Gui(opts) => {
            if cli::lacks_display_here() {
                eprintln!("{}", cli::NO_DISPLAY_MESSAGE);
                return ExitCode::FAILURE;
            }
            yhtye_lib::run_app(&opts);
            ExitCode::SUCCESS
        }
    }
}
