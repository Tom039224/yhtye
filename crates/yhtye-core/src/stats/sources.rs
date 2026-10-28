//! Where each harness keeps its session logs on this machine, with the same
//! environment overrides as agentsview (the project these statistics are
//! modelled on).

use std::ffi::OsString;
use std::path::PathBuf;

/// The log locations. A location that does not exist is not an error: that
/// harness just contributes nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatsSources {
    /// Claude Code: `<dir>/<encoded cwd>/<session>.jsonl`.
    pub claude_projects: Option<PathBuf>,
    /// Codex: rollout files anywhere below these (`sessions`, `archived_sessions`).
    pub codex_dirs: Vec<PathBuf>,
    /// OpenCode: the SQLite database.
    pub opencode_db: Option<PathBuf>,
}

impl StatsSources {
    /// No source at all (tests that must not read the machine's logs).
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The locations of this process's user: `CLAUDE_PROJECTS_DIR`,
    /// `CLAUDE_CONFIG_DIR/projects` or `~/.claude/projects`; `CODEX_SESSIONS_DIR`
    /// or `$CODEX_HOME/sessions` (`~/.codex/sessions`) and `archived_sessions`
    /// next to it; `OPENCODE_DIR/opencode.db` or `~/.local/share/opencode`.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_vars(&|k| std::env::var_os(k))
    }

    pub(super) fn from_vars(env: &dyn Fn(&str) -> Option<OsString>) -> Self {
        let non_empty = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = non_empty("HOME").or_else(|| non_empty("USERPROFILE"));
        let under_home = |rel: &str| home.as_ref().map(|h| h.join(rel));

        let claude_projects = non_empty("CLAUDE_PROJECTS_DIR")
            .or_else(|| non_empty("CLAUDE_CONFIG_DIR").map(|d| d.join("projects")))
            .or_else(|| under_home(".claude/projects"));

        let codex_home = non_empty("CODEX_HOME").or_else(|| under_home(".codex"));
        let mut codex_dirs = Vec::new();
        match non_empty("CODEX_SESSIONS_DIR") {
            Some(dir) => codex_dirs.push(dir),
            None => codex_dirs.extend(codex_home.as_ref().map(|h| h.join("sessions"))),
        }
        codex_dirs.extend(codex_home.map(|h| h.join("archived_sessions")));

        let opencode_db = non_empty("OPENCODE_DIR")
            .or_else(|| non_empty("XDG_DATA_HOME").map(|d| d.join("opencode")))
            .or_else(|| under_home(".local/share/opencode"))
            .map(|d| d.join("opencode.db"));

        Self {
            claude_projects,
            codex_dirs,
            opencode_db,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(vars: &[(&str, &str)]) -> StatsSources {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        StatsSources::from_vars(&|k| {
            vars.iter()
                .find(|(name, _)| name == k)
                .map(|(_, v)| OsString::from(v))
        })
    }

    #[test]
    fn defaults_live_under_the_home_directory() {
        let s = sources(&[("HOME", "/h")]);
        assert_eq!(s.claude_projects, Some("/h/.claude/projects".into()));
        assert_eq!(
            s.codex_dirs,
            [
                PathBuf::from("/h/.codex/sessions"),
                PathBuf::from("/h/.codex/archived_sessions")
            ]
        );
        assert_eq!(
            s.opencode_db,
            Some("/h/.local/share/opencode/opencode.db".into())
        );
    }

    #[test]
    fn environment_overrides_win() {
        let s = sources(&[
            ("HOME", "/h"),
            ("CLAUDE_CONFIG_DIR", "/cfg"),
            ("CODEX_HOME", "/cx"),
            ("OPENCODE_DIR", "/oc"),
        ]);
        assert_eq!(s.claude_projects, Some("/cfg/projects".into()));
        assert_eq!(
            s.codex_dirs,
            [
                PathBuf::from("/cx/sessions"),
                PathBuf::from("/cx/archived_sessions")
            ]
        );
        assert_eq!(s.opencode_db, Some("/oc/opencode.db".into()));

        let s = sources(&[
            ("HOME", "/h"),
            ("CLAUDE_PROJECTS_DIR", "/p"),
            ("CLAUDE_CONFIG_DIR", "/cfg"),
            ("CODEX_SESSIONS_DIR", "/rollouts"),
        ]);
        assert_eq!(s.claude_projects, Some("/p".into()));
        assert_eq!(s.codex_dirs[0], PathBuf::from("/rollouts"));
    }

    #[test]
    fn without_a_home_there_is_nothing_to_read() {
        assert_eq!(sources(&[]), StatsSources::none());
        assert_eq!(sources(&[("HOME", "")]), StatsSources::none());
    }
}
