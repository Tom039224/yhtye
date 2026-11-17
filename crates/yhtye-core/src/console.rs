//! Helper programs (git, gh, tailscale) started without a console window.
//!
//! On Windows the app is a GUI program with no console, so every console
//! program it starts gets a console of its own, shown as a window that flashes
//! up (and takes a taskbar slot) for each `git` the UI's polling runs; creating
//! those windows also makes each run much slower. `CREATE_NO_WINDOW` gives the
//! child a hidden console instead. Agents get the same flag where their process
//! tree is set up (`acp::process`). Elsewhere this does nothing.

/// [`NoConsoleWindow::no_console_window`] for `std` and `tokio` commands.
pub trait NoConsoleWindow {
    /// Starts the program without a console window (Windows; a no-op elsewhere).
    fn no_console_window(&mut self) -> &mut Self;
}

#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

impl NoConsoleWindow for std::process::Command {
    fn no_console_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(self, CREATE_NO_WINDOW);
        self
    }
}

impl NoConsoleWindow for tokio::process::Command {
    fn no_console_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(CREATE_NO_WINDOW);
        self
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::NoConsoleWindow;

    /// PowerShell printing its `GetConsoleWindow()`: 0 when its console has no window.
    const PRINT_CONSOLE_WINDOW: &str = "(Add-Type -Name W -Namespace Yhtye -PassThru -MemberDefinition \
        '[DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow();')::GetConsoleWindow()";

    fn console_window(out: std::process::Output) -> String {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn a_std_command_gets_no_console_window() {
        let out = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                PRINT_CONSOLE_WINDOW,
            ])
            .no_console_window()
            .output()
            .expect("run powershell");
        assert_eq!(console_window(out), "0");
    }

    #[tokio::test]
    async fn a_tokio_command_gets_no_console_window() {
        let out = tokio::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                PRINT_CONSOLE_WINDOW,
            ])
            .no_console_window()
            .output()
            .await
            .expect("run powershell");
        assert_eq!(console_window(out), "0");
    }
}
