//! The command line of `yhtye` (`core-design.md` §9): the desktop app by
//! default, `--headless` for the core without a GUI, and `yhtye remote …`
//! (thin clients of a running headless instance). Parsed before anything else
//! starts, so that none of them needs a display.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// How `--headless` runs relative to the terminal that started it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Daemon {
    /// In the foreground.
    #[default]
    No,
    /// `-d` / `--daemon`: start a detached copy of this process, wait until it
    /// answers, and return.
    Launch,
    /// `--daemon-child` (internal: what `-d` starts): the detached copy itself.
    Child,
}

/// What the flags of the desktop app and of `--headless` set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// `-d` / `--daemon` (headless only).
    pub daemon: Daemon,
    /// `--port N`: where remote access listens (headless only; absent: the
    /// stored port, 1423 at first).
    pub port: Option<u16>,
    /// `--data-dir DIR` (else `YHTYE_DATA_DIR`, else the app data directory).
    pub data_dir: Option<PathBuf>,
    /// `--model NAME` (else `YHTYE_MODEL`).
    pub model: Option<String>,
    /// A `yhtye://…` link given as the one plain argument (what the OS passes
    /// when a link is clicked, `Exec=yhtye %u`). The desktop app only: the
    /// deep-link plugin reads it from the arguments itself, and a running app
    /// gets it from the second one (`deep_link`); parsing must just not refuse it.
    pub open_url: Option<String>,
}

/// `yhtye remote <action>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAction {
    /// Issues a pairing code and shows it (URI, QR code, host/port/code).
    Pair,
    /// Lists the paired devices.
    Devices,
    /// Forgets a paired device.
    Revoke(String),
    /// Shows whether Yhtye runs headless (and its pid) and whether remote
    /// access is listening, and where.
    Status,
    /// Stops the running headless instance (after its agents have stopped).
    Stop,
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// The desktop app.
    Gui(Options),
    /// The core and remote access without a GUI.
    Headless(Options),
    /// A thin client of a running headless instance.
    Remote {
        action: RemoteAction,
        data_dir: Option<PathBuf>,
    },
    Help,
}

/// What a link the OS opens the app with starts with (`tauri.conf.json`: `plugins.deep-link.desktop.schemes`).
const LINK_PREFIX: &str = "yhtye://";

/// What `--help` prints.
pub const USAGE: &str = "\
usage: yhtye [--data-dir DIR] [--model NAME] [yhtye://pair?…]
       yhtye --headless [-d] [--port N] [--data-dir DIR] [--model NAME]
       yhtye remote <pair | devices | revoke <id> | status | stop> [--data-dir DIR]

  (none)          open the desktop app (given a yhtye://pair?… link: ask to pair with that host)
  --headless      run the core and remote access without a GUI (for a server;
                  stop it with SIGINT / SIGTERM)
  -d, --daemon    with --headless: detach from the terminal and keep running after it
                  closes (log: <data dir>/headless.log; stop: `yhtye remote stop`)
  --port N        port of remote access (default: the stored one, else 1423)
  --data-dir DIR  database and worktrees (default: $YHTYE_DATA_DIR, else
                  $XDG_DATA_HOME/io.github.tom039224.yhtye)
  --model NAME    model of every role (default: $YHTYE_MODEL, else haiku)

  remote pair     issue a pairing code for the Android app (URI, QR code, host / port / code)
  remote devices  list the paired devices
  remote revoke   forget a paired device
  remote status   show whether Yhtye runs headless (pid) and whether remote access is listening
  remote stop     stop the running headless Yhtye (waits for its agents to stop)
                  (these talk to a running `yhtye --headless` on the same data directory)

  -h, --help      show this help
";

/// A command line that cannot be run; the message says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

fn error<T>(message: impl Into<String>) -> Result<T, UsageError> {
    Err(UsageError(message.into()))
}

/// Parses the arguments after the program name.
pub fn parse<I>(args: I) -> Result<Invocation, UsageError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = Args::new(args);
    if args.peek_is("remote") {
        args.next();
        return parse_remote(args);
    }
    let mut headless = false;
    let mut opts = Options::default();
    while let Some(arg) = args.next() {
        let Some(flag) = arg.name() else {
            if arg.is_yhtye_link() && opts.open_url.is_none() {
                opts.open_url = Some(arg.display());
                continue;
            }
            return error(format!("unknown argument {}", arg.display()));
        };
        match flag {
            "-h" | "--help" => return Ok(Invocation::Help),
            "--headless" => {
                arg.no_value()?;
                headless = true;
            }
            "-d" | "--daemon" => {
                arg.no_value()?;
                // `--daemon-child` is what a launch starts; it stays a child.
                if opts.daemon == Daemon::No {
                    opts.daemon = Daemon::Launch;
                }
            }
            "--daemon-child" => {
                arg.no_value()?;
                opts.daemon = Daemon::Child;
            }
            "--port" => opts.port = Some(parse_port(&arg.value(&mut args)?)?),
            "--data-dir" => opts.data_dir = Some(arg.value(&mut args)?.into()),
            "--model" => opts.model = Some(arg.text_value(&mut args)?),
            other => return error(format!("unknown argument {other}")),
        }
    }
    if headless {
        if opts.open_url.is_some() {
            return error("a yhtye:// link is opened by the desktop app, not by --headless");
        }
        return Ok(Invocation::Headless(opts));
    }
    if opts.port.is_some() {
        return error("--port needs --headless (the desktop app keeps the port in its settings)");
    }
    if opts.daemon != Daemon::No {
        return error("--daemon needs --headless (the desktop app opens a window)");
    }
    Ok(Invocation::Gui(opts))
}

fn parse_remote(mut args: Args) -> Result<Invocation, UsageError> {
    let mut words = Vec::new();
    let mut data_dir = None;
    while let Some(arg) = args.next() {
        match arg.name() {
            Some("-h" | "--help") => return Ok(Invocation::Help),
            Some("--data-dir") => data_dir = Some(arg.value(&mut args)?.into()),
            Some(other) => return error(format!("unknown argument {other} for `remote`")),
            None => words.push(arg.display()),
        }
    }
    let action = match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => return error("`remote` needs pair, devices, revoke <id>, status or stop"),
        ["pair"] => RemoteAction::Pair,
        ["devices"] => RemoteAction::Devices,
        ["status"] => RemoteAction::Status,
        ["stop"] => RemoteAction::Stop,
        ["revoke"] => return error("`remote revoke` needs a device id (see `remote devices`)"),
        ["revoke", id] => RemoteAction::Revoke(id.to_string()),
        [first, ..] if matches!(first, "pair" | "devices" | "status" | "stop" | "revoke") => {
            return error(format!("too many arguments for `remote {first}`"));
        }
        [first, ..] => return error(format!("unknown remote command {first}")),
    };
    Ok(Invocation::Remote { action, data_dir })
}

fn parse_port(text: &OsStr) -> Result<u16, UsageError> {
    match text.to_str().and_then(|t| t.parse::<u16>().ok()) {
        Some(port) if port != 0 => Ok(port),
        _ => error(format!("bad port {} (1-65535)", text.to_string_lossy())),
    }
}

/// The arguments, with `--flag=value` split into the flag and its value.
struct Args {
    rest: std::vec::IntoIter<OsString>,
    peeked: Option<OsString>,
}

impl Args {
    fn new<I: IntoIterator<Item = OsString>>(args: I) -> Self {
        Self {
            rest: args.into_iter().collect::<Vec<_>>().into_iter(),
            peeked: None,
        }
    }

    fn peek_is(&mut self, word: &str) -> bool {
        if self.peeked.is_none() {
            self.peeked = self.rest.next();
        }
        self.peeked.as_deref() == Some(OsStr::new(word))
    }

    fn next(&mut self) -> Option<Arg> {
        self.peeked
            .take()
            .or_else(|| self.rest.next())
            .map(Arg::new)
    }
}

/// One argument: `--flag`, `--flag=value` or a plain word.
struct Arg {
    raw: OsString,
    /// The part before `=` of a `--flag=value`.
    flag: Option<String>,
    inline_value: Option<OsString>,
}

impl Arg {
    fn new(raw: OsString) -> Self {
        let text = raw.to_str();
        let split = text
            .filter(|t| t.starts_with("--"))
            .and_then(|t| t.split_once('='));
        let (flag, inline_value) = match split {
            Some((flag, value)) => (Some(flag.to_string()), Some(OsString::from(value))),
            None => (None, None),
        };
        Self {
            raw,
            flag,
            inline_value,
        }
    }

    /// The flag (`--port`), or `None` for a word that is not one.
    fn name(&self) -> Option<&str> {
        if let Some(flag) = &self.flag {
            return Some(flag);
        }
        self.raw.to_str().filter(|t| t.starts_with('-'))
    }

    fn display(&self) -> String {
        self.raw.to_string_lossy().into_owned()
    }

    /// A `yhtye://…` link (the scheme the OS hands to the app).
    fn is_yhtye_link(&self) -> bool {
        self.raw.to_str().is_some_and(|t| {
            t.get(..LINK_PREFIX.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(LINK_PREFIX))
        })
    }

    /// The flag takes no value.
    fn no_value(&self) -> Result<(), UsageError> {
        match &self.inline_value {
            Some(_) => error(format!(
                "{} takes no value",
                self.flag.as_deref().unwrap_or("")
            )),
            None => Ok(()),
        }
    }

    /// The value of the flag: after `=`, else the next argument.
    fn value(&self, args: &mut Args) -> Result<OsString, UsageError> {
        let name = self.name().unwrap_or_default();
        let value = match &self.inline_value {
            Some(v) => Some(v.clone()),
            None => args.next().map(|a| a.raw),
        };
        match value {
            Some(v) if !v.is_empty() => Ok(v),
            _ => error(format!("{name} needs a value")),
        }
    }

    fn text_value(&self, args: &mut Args) -> Result<String, UsageError> {
        let value = self.value(args)?;
        value
            .into_string()
            .map_err(|v| UsageError(format!("{} is not valid text", v.to_string_lossy())))
    }
}

/// Whether the desktop app has nowhere to open: no X11 / Wayland display on a
/// Unix desktop OS (`DISPLAY` and `WAYLAND_DISPLAY`, empty counting as unset).
/// Always `false` where there is no such variable (macOS, Windows).
#[must_use]
pub fn lacks_display(display: Option<&OsStr>, wayland_display: Option<&OsStr>) -> bool {
    if cfg!(not(all(unix, not(target_os = "macos")))) {
        return false;
    }
    let unset = |v: Option<&OsStr>| v.is_none_or(OsStr::is_empty);
    unset(display) && unset(wayland_display)
}

/// [`lacks_display`] of this process.
#[must_use]
pub fn lacks_display_here() -> bool {
    lacks_display(
        std::env::var_os("DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
    )
}

/// What to say when the app cannot be shown.
pub const NO_DISPLAY_MESSAGE: &str = "\
Yhtye: GUI を表示できません (DISPLAY も WAYLAND_DISPLAY もありません)。\
GUI の無い環境では `yhtye --headless` を使ってください。
Yhtye: no display to open the app on (neither DISPLAY nor WAYLAND_DISPLAY is set). \
Use `yhtye --headless` on a machine without a GUI.";

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Invocation, UsageError> {
        parse(args.iter().map(OsString::from))
    }

    fn opts(port: Option<u16>, data_dir: Option<&str>, model: Option<&str>) -> Options {
        Options {
            daemon: Daemon::No,
            port,
            data_dir: data_dir.map(PathBuf::from),
            model: model.map(str::to_string),
            open_url: None,
        }
    }

    #[test]
    fn no_arguments_open_the_desktop_app() {
        assert_eq!(parse_strs(&[]), Ok(Invocation::Gui(Options::default())));
    }

    #[test]
    fn headless_takes_the_port_the_data_dir_and_the_model() {
        assert_eq!(
            parse_strs(&["--headless"]),
            Ok(Invocation::Headless(Options::default()))
        );
        assert_eq!(
            parse_strs(&[
                "--headless",
                "--port",
                "1500",
                "--data-dir",
                "/srv/yhtye",
                "--model",
                "sonnet"
            ]),
            Ok(Invocation::Headless(opts(
                Some(1500),
                Some("/srv/yhtye"),
                Some("sonnet")
            )))
        );
    }

    #[test]
    fn a_flag_may_carry_its_value_after_an_equals_sign() {
        assert_eq!(
            parse_strs(&["--port=1500", "--headless", "--data-dir=/srv/y=z"]),
            Ok(Invocation::Headless(opts(
                Some(1500),
                Some("/srv/y=z"),
                None
            )))
        );
    }

    #[test]
    fn the_desktop_app_takes_the_data_dir_and_the_model_but_no_port() {
        assert_eq!(
            parse_strs(&["--data-dir", "/tmp/y", "--model", "opus"]),
            Ok(Invocation::Gui(opts(None, Some("/tmp/y"), Some("opus"))))
        );
        let err = parse_strs(&["--port", "1500"]).unwrap_err();
        assert!(err.0.contains("--headless"), "{err}");
    }

    #[test]
    fn headless_may_detach_with_d_or_daemon() {
        for flag in ["-d", "--daemon"] {
            let Ok(Invocation::Headless(o)) = parse_strs(&["--headless", flag, "--port", "1500"])
            else {
                panic!("headless");
            };
            assert_eq!(o.daemon, Daemon::Launch, "{flag}");
            assert_eq!(o.port, Some(1500));
        }
        // The flag may come before `--headless` too, and the child stays a child.
        let Ok(Invocation::Headless(o)) = parse_strs(&["-d", "--headless"]) else {
            panic!("headless");
        };
        assert_eq!(o.daemon, Daemon::Launch);
        let Ok(Invocation::Headless(o)) = parse_strs(&["--headless", "--daemon-child", "-d"])
        else {
            panic!("headless");
        };
        assert_eq!(o.daemon, Daemon::Child);
    }

    #[test]
    fn detaching_needs_headless_and_takes_no_value() {
        for args in [
            &["-d"][..],
            &["--daemon"],
            &["--daemon-child"],
            &["--headless", "--daemon=1"],
        ] {
            assert!(parse_strs(args).is_err(), "{args:?}");
        }
        assert!(parse_strs(&["-d"]).unwrap_err().0.contains("--headless"));
    }

    #[test]
    fn a_bad_port_or_a_missing_value_is_refused() {
        for args in [
            &["--headless", "--port", "0"][..],
            &["--headless", "--port", "70000"],
            &["--headless", "--port", "x"],
            &["--headless", "--port"],
            &["--headless", "--data-dir"],
            &["--headless", "--model="],
            &["--headless=1"],
        ] {
            assert!(parse_strs(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn a_yhtye_link_opens_the_desktop_app_with_it() {
        let link = "yhtye://pair?v=1&ip=100.64.0.1&port=1423&code=ABCD2345";
        let Ok(Invocation::Gui(o)) = parse_strs(&[link]) else {
            panic!("gui");
        };
        assert_eq!(o.open_url.as_deref(), Some(link));
        // Together with the flags of the app, and in any case.
        let Ok(Invocation::Gui(o)) = parse_strs(&["--data-dir", "/tmp/y", "YHTYE://pair?v=1"])
        else {
            panic!("gui");
        };
        assert_eq!(o.open_url.as_deref(), Some("YHTYE://pair?v=1"));
        assert_eq!(o.data_dir, Some(PathBuf::from("/tmp/y")));
    }

    #[test]
    fn a_yhtye_link_is_not_for_headless_and_only_one_is_taken() {
        let err = parse_strs(&["--headless", "yhtye://pair?v=1"]).unwrap_err();
        assert!(err.0.contains("desktop app"), "{err}");
        assert!(parse_strs(&["yhtye://pair?v=1", "yhtye://pair?v=2"]).is_err());
        assert!(parse_strs(&["remote", "yhtye://pair?v=1"]).is_err());
        // Another scheme is as unknown as any word.
        assert!(parse_strs(&["https://example.com"]).is_err());
    }

    #[test]
    fn an_unknown_argument_is_refused_instead_of_opening_the_app() {
        assert!(parse_strs(&["--headles"])
            .unwrap_err()
            .0
            .contains("--headles"));
        assert!(parse_strs(&["file.txt"]).is_err());
    }

    #[test]
    fn help_wins_wherever_it_stands() {
        assert_eq!(parse_strs(&["--help"]), Ok(Invocation::Help));
        assert_eq!(parse_strs(&["--headless", "-h"]), Ok(Invocation::Help));
        assert_eq!(parse_strs(&["remote", "--help"]), Ok(Invocation::Help));
    }

    #[test]
    fn the_remote_subcommands_are_parsed() {
        let remote = |action| Invocation::Remote {
            action,
            data_dir: None,
        };
        assert_eq!(
            parse_strs(&["remote", "pair"]),
            Ok(remote(RemoteAction::Pair))
        );
        assert_eq!(
            parse_strs(&["remote", "devices"]),
            Ok(remote(RemoteAction::Devices))
        );
        assert_eq!(
            parse_strs(&["remote", "status"]),
            Ok(remote(RemoteAction::Status))
        );
        assert_eq!(
            parse_strs(&["remote", "stop"]),
            Ok(remote(RemoteAction::Stop))
        );
        assert_eq!(
            parse_strs(&["remote", "revoke", "dev-1"]),
            Ok(remote(RemoteAction::Revoke("dev-1".into())))
        );
        assert_eq!(
            parse_strs(&["remote", "--data-dir", "/tmp/y", "pair"]),
            Ok(Invocation::Remote {
                action: RemoteAction::Pair,
                data_dir: Some(PathBuf::from("/tmp/y")),
            })
        );
    }

    #[test]
    fn a_remote_command_that_does_not_fit_is_refused() {
        for args in [
            &["remote"][..],
            &["remote", "revoke"],
            &["remote", "pair", "extra"],
            &["remote", "stop", "now"],
            &["remote", "forget"],
            &["remote", "--port", "1"],
            &["remote", "pair", "--headless"],
        ] {
            assert!(parse_strs(args).is_err(), "{args:?}");
        }
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn the_app_has_no_display_when_neither_variable_is_set() {
        let some = |s: &'static str| Some(OsStr::new(s));
        assert!(lacks_display(None, None));
        assert!(lacks_display(some(""), some("")), "empty counts as unset");
        assert!(!lacks_display(some(":0"), None));
        assert!(!lacks_display(None, some("wayland-0")));
        assert!(!lacks_display(some(":0"), some("wayland-0")));
    }
}
