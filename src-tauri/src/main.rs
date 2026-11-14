// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use yhtye_lib::cli::{self, Invocation};

/// A release build on Windows is a GUI program with no console, so what the
/// command line mode prints (usage, errors, `yhtye remote …`) would go nowhere.
/// Attaching the parent's console, if there is one, makes it appear there.
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: a plain Win32 call without pointers; it fails harmlessly (and is
    // ignored) when the process has no parent console, e.g. a double click.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
fn attach_parent_console() {}

/// Looks at the command line before anything else starts: `--headless` and
/// `yhtye remote …` must run where there is no display (GTK is only
/// initialised once the window is built), and the app must say so instead of
/// panicking when there is none.
fn main() -> ExitCode {
    let invocation = match cli::parse(std::env::args_os().skip(1)) {
        Ok(invocation) => invocation,
        Err(e) => {
            attach_parent_console();
            eprintln!("yhtye: {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match invocation {
        Invocation::Help => {
            attach_parent_console();
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Invocation::Headless(opts) => {
            attach_parent_console();
            yhtye_lib::headless::run(opts)
        }
        Invocation::Remote { action, data_dir } => {
            attach_parent_console();
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
