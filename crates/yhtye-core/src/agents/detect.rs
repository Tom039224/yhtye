//! Finding the harnesses' executables (`core-design.md` §15.2): a search of
//! `PATH` (then a few well-known directories), or a path the user gave by hand.
//! Nothing is run (no `--version`): a file that is executable is enough. Pure
//! functions over an injected environment so they can be tested.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

use super::catalog::{CLAUDE_CODE, CODEX, DEVIN, OPENCODE};

/// The OpenCode executable.
pub const OPENCODE_COMMAND: &str = "opencode";

/// The Codex executable (the adapter runs it through `CODEX_PATH`; the adapter
/// itself is fetched by `npx`).
pub const CODEX_COMMAND: &str = "codex";

/// Runs the Claude Code and Codex adapters (`npx -y <adapter>`).
pub const NPX_COMMAND: &str = "npx";

/// The Devin executable.
pub const DEVIN_COMMAND: &str = "devin";

/// Directories searched after `PATH` (a desktop app often starts with a shorter
/// `PATH` than the user's shell has). `~` is the `HOME` of the environment.
pub const KNOWN_DIRS: [&str; 4] = [
    "~/.local/bin",
    "~/.cargo/bin",
    "~/.bun/bin",
    "/usr/local/bin",
];

/// What a harness needs to be offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessSpec {
    /// The harness id (`claude-code`).
    pub id: &'static str,
    pub label: &'static str,
    /// The main executable: the one a manual path replaces, and the one shown
    /// as the harness's path.
    pub main: &'static str,
    /// Further commands it needs (found automatically only).
    pub also: &'static [&'static str],
}

/// The known harnesses, in the order the built-in default prefers them
/// ([`super::default_choice`] has the rule).
pub const HARNESS_SPECS: [HarnessSpec; 4] = [
    HarnessSpec {
        id: CLAUDE_CODE,
        label: "Claude Code",
        main: NPX_COMMAND,
        also: &[],
    },
    HarnessSpec {
        id: OPENCODE,
        label: "OpenCode",
        main: OPENCODE_COMMAND,
        also: &[],
    },
    HarnessSpec {
        id: CODEX,
        label: "Codex",
        main: CODEX_COMMAND,
        also: &[NPX_COMMAND],
    },
    HarnessSpec {
        id: DEVIN,
        label: "Devin",
        main: DEVIN_COMMAND,
        also: &[],
    },
];

/// Where an executable was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "HarnessPathSource")]
pub enum PathSource {
    /// The path the user gave.
    Override,
    /// A directory of `PATH`.
    Path,
    /// One of [`KNOWN_DIRS`].
    KnownDir,
    /// Not found.
    None,
}

/// One command a harness needs, and where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct HarnessRequirement {
    pub command: String,
    /// The absolute path, `None` when not found.
    pub found: Option<String>,
}

/// Whether a harness can be used, and why (the settings panel shows it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct HarnessDetection {
    pub id: String,
    pub label: String,
    /// Every command it needs was found (and a manual path, if any, is usable).
    pub installed: bool,
    /// The main executable ([`HarnessSpec::main`]); `None` when not found.
    pub resolved_path: Option<String>,
    pub path_source: PathSource,
    /// The path the user gave, even when it is unusable.
    pub override_path: Option<String>,
    /// Why `override_path` cannot be used. The harness is then not installed:
    /// the search does not replace a manual path that is wrong.
    pub override_error: Option<String>,
    pub requirements: Vec<HarnessRequirement>,
}

impl HarnessDetection {
    /// Where `command` was found, if it is one of the requirements.
    #[must_use]
    pub fn found(&self, command: &str) -> Option<&str> {
        self.requirements
            .iter()
            .find(|r| r.command == command)
            .and_then(|r| r.found.as_deref())
    }
}

/// The spec of the harness `id`.
#[must_use]
pub fn harness_spec(id: &str) -> Option<&'static HarnessSpec> {
    HARNESS_SPECS.iter().find(|s| s.id == id)
}

/// [`KNOWN_DIRS`] with `~` expanded to the `HOME` of `env` (those under `~`
/// are left out without one).
pub fn known_dirs(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = env("HOME").map(PathBuf::from).filter(|h| h.is_absolute());
    KNOWN_DIRS
        .iter()
        .filter_map(|dir| match dir.strip_prefix("~/") {
            Some(rest) => home.as_ref().map(|h| h.join(rest)),
            None => Some(PathBuf::from(dir)),
        })
        .collect()
}

/// The first executable regular file named `command` in the directories of
/// `path` (a `PATH` value). Relative entries are ignored.
#[must_use]
pub fn find_in_path(command: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(command))
        .find(|p| is_executable(p))
}

/// `command` in `PATH` of `env`, else in `known_dirs`. A path that is not
/// valid UTF-8 cannot be shown or passed on, so it does not count.
pub fn find_command(
    command: &str,
    env: impl Fn(&str) -> Option<OsString>,
    known_dirs: &[PathBuf],
) -> Option<(PathBuf, PathSource)> {
    let usable = |p: &PathBuf| p.to_str().is_some();
    if let Some(path) = find_in_path(command, env("PATH").as_deref()).filter(usable) {
        return Some((path, PathSource::Path));
    }
    known_dirs
        .iter()
        .map(|dir| dir.join(command))
        .find(|p| is_executable(p) && usable(p))
        .map(|p| (p, PathSource::KnownDir))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Why `path` cannot be a manual executable path: it must be absolute and an
/// executable regular file.
pub fn check_executable_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if path.is_empty() {
        return Err("the path is empty".into());
    }
    if !p.is_absolute() {
        return Err(format!("{path} is not an absolute path"));
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a regular file"));
    }
    if !is_executable(p) {
        return Err(format!("{path} is not executable"));
    }
    Ok(())
}

/// The state of every known harness, in [`HARNESS_SPECS`] order. `overrides`
/// maps a harness id to the path the user gave for its main executable, which
/// is used instead of the search (and is not replaced by it when it is wrong).
pub fn detect_harnesses(
    env: impl Fn(&str) -> Option<OsString>,
    known_dirs: &[PathBuf],
    overrides: &HashMap<String, String>,
) -> Vec<HarnessDetection> {
    HARNESS_SPECS
        .iter()
        .map(|spec| detect(spec, &env, known_dirs, overrides.get(spec.id)))
        .collect()
}

fn detect(
    spec: &HarnessSpec,
    env: &impl Fn(&str) -> Option<OsString>,
    known_dirs: &[PathBuf],
    override_path: Option<&String>,
) -> HarnessDetection {
    let (main, path_source, override_error) = match override_path {
        Some(path) => match check_executable_path(path) {
            Ok(()) => (Some(path.clone()), PathSource::Override, None),
            Err(e) => (None, PathSource::None, Some(e)),
        },
        None => match find_command(spec.main, env, known_dirs) {
            Some((path, source)) => (path.to_str().map(str::to_string), source, None),
            None => (None, PathSource::None, None),
        },
    };
    let mut requirements = vec![HarnessRequirement {
        command: spec.main.to_string(),
        found: main.clone(),
    }];
    requirements.extend(spec.also.iter().map(|command| {
        HarnessRequirement {
            command: (*command).to_string(),
            found: find_command(command, env, known_dirs)
                .and_then(|(path, _)| path.to_str().map(str::to_string)),
        }
    }));
    HarnessDetection {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        installed: requirements.iter().all(|r| r.found.is_some()),
        resolved_path: main,
        path_source,
        override_path: override_path.cloned(),
        override_error,
        requirements,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn fake_bin(dir: &Path, name: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).expect("chmod");
        p
    }

    fn path_of(dir: &tempfile::TempDir) -> String {
        dir.path().display().to_string()
    }

    fn by_id<'a>(all: &'a [HarnessDetection], id: &str) -> &'a HarnessDetection {
        all.iter().find(|d| d.id == id).expect("known harness")
    }

    fn detect_all(path: &str, overrides: &[(&str, &str)]) -> Vec<HarnessDetection> {
        let overrides = overrides
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        detect_harnesses(env(&[("PATH", path)]), &[], &overrides)
    }

    #[test]
    fn a_command_is_found_only_as_an_executable_file_in_an_absolute_path_entry() {
        let with = tempfile::tempdir().expect("tempdir");
        let without = tempfile::tempdir().expect("tempdir");
        let bin = fake_bin(with.path(), "opencode", 0o755);
        fake_bin(without.path(), "opencode", 0o644); // not executable
        std::fs::create_dir(with.path().join("dir")).expect("mkdir");
        let find = |path: &str| find_command("opencode", env(&[("PATH", path)]), &[]);

        assert_eq!(find(&path_of(&with)), Some((bin.clone(), PathSource::Path)));
        assert_eq!(find(&path_of(&without)), None, "not executable");
        assert_eq!(
            find(&format!("relative:{}", path_of(&with))),
            Some((bin, PathSource::Path))
        );
        assert_eq!(find("relative"), None, "relative PATH entries are ignored");
        assert_eq!(
            find_command("dir", env(&[("PATH", &path_of(&with))]), &[]),
            None,
            "a directory is not a command"
        );
        assert_eq!(find_command("opencode", env(&[]), &[]), None, "no PATH");
    }

    #[test]
    fn the_path_wins_over_the_known_directories() {
        let on_path = tempfile::tempdir().expect("tempdir");
        let known = tempfile::tempdir().expect("tempdir");
        fake_bin(on_path.path(), "devin", 0o755);
        fake_bin(known.path(), "devin", 0o755);
        let dirs = [known.path().to_path_buf()];
        let found = find_command("devin", env(&[("PATH", &path_of(&on_path))]), &dirs);
        assert_eq!(
            found,
            Some((on_path.path().join("devin"), PathSource::Path))
        );
        let found = find_command("devin", env(&[("PATH", "/nonexistent")]), &dirs);
        assert_eq!(
            found,
            Some((known.path().join("devin"), PathSource::KnownDir))
        );
    }

    #[test]
    fn known_directories_expand_the_home_of_the_environment() {
        let dirs = known_dirs(env(&[("HOME", "/home/u")]));
        assert_eq!(
            dirs,
            [
                PathBuf::from("/home/u/.local/bin"),
                PathBuf::from("/home/u/.cargo/bin"),
                PathBuf::from("/home/u/.bun/bin"),
                PathBuf::from("/usr/local/bin"),
            ]
        );
        assert_eq!(
            known_dirs(env(&[])),
            [PathBuf::from("/usr/local/bin")],
            "no HOME: only the absolute directories"
        );
        assert_eq!(known_dirs(env(&[("HOME", "relative")])).len(), 1);
    }

    #[test]
    fn every_harness_is_reported_with_what_it_needs() {
        let bin = tempfile::tempdir().expect("tempdir");
        for name in ["npx", "opencode", "codex", "devin"] {
            fake_bin(bin.path(), name, 0o755);
        }
        let all = detect_all(&path_of(&bin), &[]);
        let ids: Vec<&str> = all.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["claude-code", "opencode", "codex", "devin"]);
        assert!(all.iter().all(|d| d.installed));

        let cc = by_id(&all, "claude-code");
        assert_eq!(cc.label, "Claude Code");
        assert_eq!(
            cc.resolved_path.as_deref(),
            Some(bin.path().join("npx").to_str().expect("utf-8"))
        );
        assert_eq!(cc.path_source, PathSource::Path);
        assert_eq!(cc.requirements.len(), 1);
        assert_eq!(cc.requirements[0].command, "npx");

        let codex = by_id(&all, "codex");
        assert_eq!(
            codex.resolved_path.as_deref(),
            Some(bin.path().join("codex").to_str().expect("utf-8")),
            "the main executable, not npx"
        );
        let commands: Vec<&str> = codex
            .requirements
            .iter()
            .map(|r| r.command.as_str())
            .collect();
        assert_eq!(commands, ["codex", "npx"]);
        assert_eq!(
            codex.found("npx"),
            Some(bin.path().join("npx").to_str().expect("utf-8"))
        );
        assert_eq!(codex.override_path, None);
        assert_eq!(codex.override_error, None);
    }

    #[test]
    fn nothing_found_means_nothing_installed() {
        let empty = tempfile::tempdir().expect("tempdir");
        let all = detect_all(&path_of(&empty), &[]);
        for d in &all {
            assert!(!d.installed, "{}", d.id);
            assert_eq!(
                (d.resolved_path.as_deref(), d.path_source),
                (None, PathSource::None)
            );
            assert!(d.requirements.iter().all(|r| r.found.is_none()));
        }
    }

    #[test]
    fn a_missing_npx_takes_claude_code_and_codex_with_it() {
        let bin = tempfile::tempdir().expect("tempdir");
        for name in ["opencode", "codex", "devin"] {
            fake_bin(bin.path(), name, 0o755);
        }
        let all = detect_all(&path_of(&bin), &[]);
        assert!(!by_id(&all, "claude-code").installed);
        let codex = by_id(&all, "codex");
        assert!(!codex.installed, "codex needs npx as well");
        assert!(codex.resolved_path.is_some(), "codex itself was found");
        assert_eq!(codex.found("npx"), None);
        assert!(by_id(&all, "opencode").installed);
        assert!(by_id(&all, "devin").installed);
    }

    #[test]
    fn devin_is_detected_by_its_own_executable() {
        let bin = tempfile::tempdir().expect("tempdir");
        fake_bin(bin.path(), "devin", 0o755);
        let all = detect_all(&path_of(&bin), &[]);
        let devin = by_id(&all, "devin");
        assert!(devin.installed);
        assert_eq!(devin.label, "Devin");
        assert!(
            !by_id(&all, "claude-code").installed,
            "claude needs npx, not a claude CLI"
        );
    }

    #[test]
    fn a_manual_path_wins_over_the_search() {
        let on_path = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        fake_bin(on_path.path(), "devin", 0o755);
        let mine = fake_bin(elsewhere.path(), "my-devin", 0o755);
        let mine = mine.to_str().expect("utf-8");
        let all = detect_all(&path_of(&on_path), &[("devin", mine)]);
        let devin = by_id(&all, "devin");
        assert!(devin.installed);
        assert_eq!(devin.resolved_path.as_deref(), Some(mine));
        assert_eq!(devin.path_source, PathSource::Override);
        assert_eq!(devin.override_path.as_deref(), Some(mine));
        assert_eq!(devin.requirements[0].found.as_deref(), Some(mine));

        // It also makes a harness installed that the search does not find.
        let none = tempfile::tempdir().expect("tempdir");
        let all = detect_all(&path_of(&none), &[("devin", mine)]);
        assert!(by_id(&all, "devin").installed);
    }

    #[test]
    fn a_manual_path_replaces_only_the_main_executable() {
        let bin = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let codex = fake_bin(elsewhere.path(), "codex", 0o755);
        let codex = codex.to_str().expect("utf-8");
        // npx is not on PATH: a path for codex does not make up for it.
        let all = detect_all(&path_of(&bin), &[("codex", codex)]);
        let d = by_id(&all, "codex");
        assert!(!d.installed);
        assert_eq!(d.resolved_path.as_deref(), Some(codex));
        assert_eq!(d.found("npx"), None);
        // claude-code's main executable is npx: a path for it counts.
        let npx = fake_bin(elsewhere.path(), "npx", 0o755);
        let npx = npx.to_str().expect("utf-8");
        let all = detect_all(&path_of(&bin), &[("claude-code", npx), ("codex", codex)]);
        assert!(by_id(&all, "claude-code").installed);
        assert!(
            !by_id(&all, "codex").installed,
            "codex's npx is detected only"
        );
    }

    #[test]
    fn a_broken_manual_path_makes_the_harness_unavailable_with_the_reason() {
        let bin = tempfile::tempdir().expect("tempdir");
        fake_bin(bin.path(), "devin", 0o755);
        let not_exec = fake_bin(bin.path(), "plain", 0o644);
        let cases = [
            ("/no/such/devin", "No such file"),
            ("relative/devin", "not an absolute path"),
            (not_exec.to_str().expect("utf-8"), "not executable"),
            (bin.path().to_str().expect("utf-8"), "not a regular file"),
        ];
        for (path, reason) in cases {
            let all = detect_all(&path_of(&bin), &[("devin", path)]);
            let d = by_id(&all, "devin");
            assert!(!d.installed, "{path}: the PATH search does not stand in");
            assert_eq!(d.resolved_path, None);
            assert_eq!(d.path_source, PathSource::None);
            assert_eq!(d.override_path.as_deref(), Some(path));
            let error = d.override_error.as_deref().expect("a reason");
            assert!(error.contains(reason), "{path}: {error}");
        }
    }

    #[test]
    fn a_harness_in_a_known_directory_reports_where_it_was_found() {
        let known = tempfile::tempdir().expect("tempdir");
        fake_bin(known.path(), "opencode", 0o755);
        let dirs = [known.path().to_path_buf()];
        let all = detect_harnesses(env(&[("PATH", "/nonexistent")]), &dirs, &HashMap::new());
        let d = by_id(&all, "opencode");
        assert!(d.installed);
        assert_eq!(d.path_source, PathSource::KnownDir);
        assert_eq!(
            d.resolved_path.as_deref(),
            Some(known.path().join("opencode").to_str().expect("utf-8"))
        );
    }

    #[test]
    fn a_manual_path_must_be_an_absolute_executable_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ok = fake_bin(dir.path(), "tool", 0o755);
        assert_eq!(check_executable_path(ok.to_str().expect("utf-8")), Ok(()));
        for bad in [
            "",
            "tool",
            "./tool",
            "/no/such/file",
            dir.path().to_str().expect("utf-8"),
            fake_bin(dir.path(), "data", 0o644).to_str().expect("utf-8"),
        ] {
            assert!(check_executable_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_spec_labels_match_the_presets() {
        use super::super::HarnessPreset;
        let presets = [
            HarnessPreset::claude_code("m"),
            HarnessPreset::opencode("m", Vec::new()),
            HarnessPreset::codex(None),
            HarnessPreset::devin("devin"),
        ];
        for (spec, preset) in HARNESS_SPECS.iter().zip(&presets) {
            assert_eq!(
                (spec.id, spec.label),
                (preset.id.as_str(), preset.label.as_str())
            );
        }
    }
}
