//! The ACP adapters of Claude Code and Codex (`docs/architecture/acp-harnesses.md`
//! §5, §9): which version `npx` runs, and which `claude` the Claude Code adapter
//! drives. The settings choose between the verified version (the default), the
//! latest one, a version typed by the user, and a global install of the adapter;
//! everything here is pure, the lookup of the latest version is in
//! [`super::adapter_latest`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::catalog::{CLAUDE_CODE, CODEX};
use super::detect::PathSource;
use crate::acp::{
    CLAUDE_AGENT_ACP_BIN, CLAUDE_AGENT_ACP_PACKAGE, CLAUDE_AGENT_ACP_VERIFIED, CODEX_ACP_BIN,
    CODEX_ACP_PACKAGE, CODEX_ACP_VERIFIED,
};

/// The longest version the settings accept (an npm version is far shorter).
const MAX_VERSION_CHARS: usize = 64;

/// An adapter Yhtye runs through `npx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdapterKind {
    /// `@agentclientprotocol/claude-agent-acp` (Claude Code).
    ClaudeAgent,
    /// `@agentclientprotocol/codex-acp` (Codex).
    Codex,
}

impl AdapterKind {
    /// Every adapter.
    pub const ALL: [Self; 2] = [Self::ClaudeAgent, Self::Codex];

    /// The npm package.
    #[must_use]
    pub const fn package(self) -> &'static str {
        match self {
            Self::ClaudeAgent => CLAUDE_AGENT_ACP_PACKAGE,
            Self::Codex => CODEX_ACP_PACKAGE,
        }
    }

    /// The version that was tested against Yhtye (the default).
    #[must_use]
    pub const fn verified(self) -> &'static str {
        match self {
            Self::ClaudeAgent => CLAUDE_AGENT_ACP_VERIFIED,
            Self::Codex => CODEX_ACP_VERIFIED,
        }
    }

    /// The executable a global install of the package puts on `PATH`.
    #[must_use]
    pub const fn global_command(self) -> &'static str {
        match self {
            Self::ClaudeAgent => CLAUDE_AGENT_ACP_BIN,
            Self::Codex => CODEX_ACP_BIN,
        }
    }

    /// The harness this adapter runs.
    #[must_use]
    pub const fn harness(self) -> &'static str {
        match self {
            Self::ClaudeAgent => CLAUDE_CODE,
            Self::Codex => CODEX,
        }
    }

    /// The adapter of `harness`, if it runs through one.
    #[must_use]
    pub fn of_harness(harness: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.harness() == harness)
    }

    /// The argument `npx -y` gets for `version`.
    #[must_use]
    pub fn spec(self, version: &str) -> String {
        format!("{}@{version}", self.package())
    }
}

/// Which adapter version a harness runs.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS,
)]
#[serde(rename_all = "snake_case")]
pub enum AdapterChannel {
    /// The version that was tested against Yhtye.
    #[default]
    Verified,
    /// The newest version npm has (looked up with `npm view`, remembered for a
    /// few hours); the verified one when it cannot be looked up.
    Latest,
    /// The version in [`HarnessOptions::adapter_version`].
    Pinned,
    /// The adapter installed globally (`claude-agent-acp` / `codex-acp` on `PATH`).
    Global,
}

/// Which Claude Code the adapter drives.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS,
)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeSource {
    /// The user's own `claude` (`CLAUDE_CODE_EXECUTABLE`), when one is found.
    #[default]
    User,
    /// The Claude Code bundled with the adapter.
    Bundled,
}

/// What the user chose for a harness besides where its executable is: the
/// adapter version and, for Claude Code, whose `claude` runs. No stored row
/// means [`HarnessOptions::default`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
pub struct HarnessOptions {
    pub adapter: AdapterChannel,
    /// The version of [`AdapterChannel::Pinned`] (semver, `1.2.3` or
    /// `1.2.3-beta.1`); `None` for the other channels.
    pub adapter_version: Option<String>,
    /// Claude Code only; `User` for the other harnesses.
    pub claude_source: ClaudeSource,
}

impl HarnessOptions {
    /// Checks options set for `harness` and returns them in their canonical
    /// form (the pinned version trimmed; no version for another channel).
    pub fn validated(&self, harness: &str) -> Result<Self, String> {
        if AdapterKind::of_harness(harness).is_none() {
            return Err(format!(
                "{harness} runs without an adapter: there is nothing to choose"
            ));
        }
        if harness != CLAUDE_CODE && self.claude_source != ClaudeSource::User {
            return Err(format!("{harness} has no Claude Code to choose"));
        }
        let adapter_version = match (self.adapter, self.adapter_version.as_deref()) {
            (AdapterChannel::Pinned, Some(version)) => Some(parse_semver(version)?),
            (AdapterChannel::Pinned, None) => {
                return Err("a version is needed to run a pinned adapter".into());
            }
            _ => None,
        };
        Ok(Self {
            adapter: self.adapter,
            adapter_version,
            claude_source: self.claude_source,
        })
    }
}

/// A version as `MAJOR.MINOR.PATCH` with an optional `-prerelease`: digits
/// without leading zeros, prerelease identifiers of `[0-9A-Za-z-]`. Build
/// metadata, a leading `v`, ranges and tags (`latest`, `^1.2.3`) are refused:
/// the text becomes part of an argument of `npx`, so only what is certainly a
/// version may pass. Returns the version without surrounding whitespace.
pub fn parse_semver(text: &str) -> Result<String, String> {
    let text = text.trim();
    let bad = || format!("{text:?} is not a version (expected like 1.2.3 or 1.2.3-beta.1)");
    if text.is_empty() || text.chars().count() > MAX_VERSION_CHARS {
        return Err(bad());
    }
    let (core, prerelease) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    if numbers.len() != 3 || !numbers.iter().all(|n| is_numeric_identifier(n)) {
        return Err(bad());
    }
    if let Some(pre) = prerelease
        && !pre.split('.').all(is_prerelease_identifier)
    {
        return Err(bad());
    }
    Ok(text.to_string())
}

/// `0` or digits without a leading zero.
fn is_numeric_identifier(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|b| b.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
}

/// A prerelease identifier: non-empty `[0-9A-Za-z-]`, a numeric one without a
/// leading zero.
fn is_prerelease_identifier(part: &str) -> bool {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return false;
    }
    !part.bytes().all(|b| b.is_ascii_digit()) || is_numeric_identifier(part)
}

/// What an adapter version resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AdapterSource {
    /// The verified version (chosen, or the fallback of another choice).
    Verified,
    /// The latest version, as npm answered.
    Latest,
    /// The version the user typed.
    Pinned,
    /// The adapter installed globally.
    Global,
}

impl AdapterSource {
    fn label(self) -> &'static str {
        match self {
            Self::Verified => "検証済み",
            Self::Latest => "最新",
            Self::Pinned => "指定",
            Self::Global => "グローバル導入",
        }
    }
}

/// The adapter a harness is going to run, and why (the settings show it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct AdapterStatus {
    /// The npm package.
    pub package: String,
    /// The version that was tested against Yhtye.
    pub verified_version: String,
    /// What it resolved to.
    pub source: AdapterSource,
    /// The version that runs; `None` for a global install (not asked).
    pub version: Option<String>,
    /// The package argument `npx -y` gets (`<package>@<version>`); `None` when
    /// the global install runs.
    pub spec: Option<String>,
    /// The global install found on `PATH`, whichever channel is chosen.
    pub global_path: Option<String>,
    /// Why the choice is not what runs (the latest version could not be
    /// looked up, no global install was found, ...).
    pub note: Option<String>,
}

impl AdapterStatus {
    /// What runs, as text: `<package>@<version>` or the global executable.
    #[must_use]
    pub fn describe(&self) -> String {
        match (&self.spec, &self.global_path, self.source) {
            (_, Some(path), AdapterSource::Global) => path.clone(),
            (Some(spec), ..) => spec.clone(),
            _ => self.package.clone(),
        }
    }
}

/// What the adapter of `kind` runs for `options`: `global_path` is the global
/// install found (if any) and `latest` the newest version known (if any). A
/// choice that cannot be followed runs the verified version, and says so in
/// [`AdapterStatus::note`].
#[must_use]
pub fn adapter_status(
    kind: AdapterKind,
    options: &HarnessOptions,
    global_path: Option<&str>,
    latest: Option<&str>,
) -> AdapterStatus {
    let verified = kind.verified();
    let verified_with = |note: Option<String>| AdapterStatus {
        package: kind.package().into(),
        verified_version: verified.into(),
        source: AdapterSource::Verified,
        version: Some(verified.into()),
        spec: Some(kind.spec(verified)),
        global_path: global_path.map(str::to_string),
        note,
    };
    let npm = |source: AdapterSource, version: &str| AdapterStatus {
        source,
        version: Some(version.into()),
        spec: Some(kind.spec(version)),
        ..verified_with(None)
    };
    match options.adapter {
        AdapterChannel::Verified => verified_with(None),
        AdapterChannel::Latest => match latest {
            Some(version) => npm(AdapterSource::Latest, version),
            None => verified_with(Some(format!(
                "最新の版を npm で調べられなかったので、検証済みの {verified} で起動します"
            ))),
        },
        AdapterChannel::Pinned => match options.adapter_version.as_deref() {
            Some(version) if parse_semver(version).is_ok() => npm(AdapterSource::Pinned, version),
            _ => verified_with(Some(format!(
                "指定された版が正しくないので、検証済みの {verified} で起動します"
            ))),
        },
        AdapterChannel::Global => match global_path {
            Some(path) => AdapterStatus {
                source: AdapterSource::Global,
                version: None,
                spec: None,
                global_path: Some(path.into()),
                ..verified_with(None)
            },
            None => verified_with(Some(format!(
                "PATH に {} が見つからないので、検証済みの {verified} で起動します",
                kind.global_command()
            ))),
        },
    }
}

/// Which `claude` the Claude Code adapter drives, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct ClaudeExecutable {
    /// The user's `claude` that was found; `None` when none was.
    pub found: Option<String>,
    pub path_source: PathSource,
    /// The `claude` the adapter is told to run (`CLAUDE_CODE_EXECUTABLE`);
    /// `None`: the Claude Code bundled with the adapter.
    pub using: Option<String>,
    /// Why the choice is not what runs.
    pub note: Option<String>,
}

/// The `claude` to run for `options`, given the one that was `found`.
#[must_use]
pub fn claude_executable(
    options: &HarnessOptions,
    found: Option<(String, PathSource)>,
) -> ClaudeExecutable {
    let (path, path_source) = match found {
        Some((path, source)) => (Some(path), source),
        None => (None, PathSource::None),
    };
    let (using, note) = match (options.claude_source, &path) {
        (ClaudeSource::User, Some(path)) => (Some(path.clone()), None),
        (ClaudeSource::User, None) => (
            None,
            Some("claude が見つからないので、アダプタ同梱の Claude Code を使います".to_string()),
        ),
        (ClaudeSource::Bundled, _) => (None, None),
    };
    ClaudeExecutable {
        found: path,
        path_source,
        using,
        note,
    }
}

/// Appended to an error of the adapter's launch or startup
/// ([`crate::acp::HarnessConfig::launch_note`]): what ran, and, when it is not
/// the verified setup, how to go back to it.
#[must_use]
pub fn launch_note(
    kind: AdapterKind,
    adapter: &AdapterStatus,
    claude: Option<&ClaudeExecutable>,
) -> String {
    let mut ran = format!(
        "アダプタ: {} ({})",
        adapter.describe(),
        adapter.source.label()
    );
    let own_claude = claude.and_then(|c| c.using.as_deref());
    if let Some(claude) = claude {
        match &claude.using {
            Some(path) => ran.push_str(&format!(" / Claude Code: {path} (自分の claude)")),
            None => ran.push_str(" / Claude Code: アダプタ同梱"),
        }
    }
    if adapter.source == AdapterSource::Verified && own_claude.is_none() {
        return ran;
    }
    let mut back = format!("アダプタを「検証済み」({})", kind.verified());
    if own_claude.is_some() {
        back.push_str("、Claude Code を「同梱」");
    }
    format!("{ran}\n設定 › ハーネス で{back}に戻すと直ることがあります。")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned(version: &str) -> HarnessOptions {
        HarnessOptions {
            adapter: AdapterChannel::Pinned,
            adapter_version: Some(version.into()),
            claude_source: ClaudeSource::User,
        }
    }

    fn channel(adapter: AdapterChannel) -> HarnessOptions {
        HarnessOptions {
            adapter,
            ..HarnessOptions::default()
        }
    }

    #[test]
    fn versions_are_accepted_only_as_strict_semver() {
        for ok in [
            "0.0.0",
            "1.2.3",
            "0.109.0",
            "10.20.30",
            "1.2.3-beta.1",
            "1.2.3-rc-1",
            "1.0.0-0",
            "1.0.0-alpha.0.x",
        ] {
            assert_eq!(parse_semver(ok), Ok(ok.to_string()), "{ok}");
        }
        assert_eq!(parse_semver("  1.2.3 \n"), Ok("1.2.3".to_string()));
        for bad in [
            "",
            " ",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "v1.2.3",
            "latest",
            "^1.2.3",
            "~1.2.3",
            ">=1.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-.x",
            "1.2.3-x..y",
            "1.2.3-01",
            "1.2.3+build",
            "1.2.3-beta+build",
            "-1.2.3",
            "1.2.3 4.5.6",
            "1.2.3; rm -rf /",
            "1.2.3 --registry=https://evil.example",
            "1.2.3\n--yes",
            "1.2.3$(id)",
            "@scope/pkg@1.2.3",
            "1.2.3-é",
            &"1".repeat(65),
        ] {
            assert!(parse_semver(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn options_are_validated_for_their_harness_and_normalized() {
        assert_eq!(
            pinned(" 0.100.0 ").validated(CLAUDE_CODE),
            Ok(pinned("0.100.0"))
        );
        // A version typed for another channel is dropped.
        let kept = HarnessOptions {
            adapter: AdapterChannel::Latest,
            adapter_version: Some("garbage".into()),
            claude_source: ClaudeSource::Bundled,
        };
        assert_eq!(
            kept.validated(CLAUDE_CODE),
            Ok(HarnessOptions {
                adapter: AdapterChannel::Latest,
                adapter_version: None,
                claude_source: ClaudeSource::Bundled,
            })
        );
        assert!(pinned("^1.0.0").validated(CODEX).is_err());
        assert!(
            HarnessOptions {
                adapter: AdapterChannel::Pinned,
                ..HarnessOptions::default()
            }
            .validated(CODEX)
            .is_err_and(|e| e.contains("version is needed"))
        );
        assert!(
            kept.validated(CODEX)
                .is_err_and(|e| e.contains("no Claude Code")),
            "Codex has no claude to choose"
        );
        assert!(
            HarnessOptions::default()
                .validated("devin")
                .is_err_and(|e| e.contains("without an adapter"))
        );
        assert_eq!(
            HarnessOptions::default().validated(CODEX),
            Ok(HarnessOptions::default())
        );
    }

    #[test]
    fn the_defaults_are_the_verified_adapter_and_the_users_claude() {
        let d = HarnessOptions::default();
        assert_eq!(d.adapter, AdapterChannel::Verified);
        assert_eq!(d.adapter_version, None);
        assert_eq!(d.claude_source, ClaudeSource::User);
    }

    #[test]
    fn each_channel_resolves_to_the_adapter_that_runs() {
        let k = AdapterKind::ClaudeAgent;
        let verified = adapter_status(k, &channel(AdapterChannel::Verified), None, Some("9.9.9"));
        assert_eq!(verified.source, AdapterSource::Verified);
        assert_eq!(
            verified.spec.as_deref(),
            Some("@agentclientprotocol/claude-agent-acp@0.84.0")
        );
        assert_eq!(verified.note, None);

        let latest = adapter_status(k, &channel(AdapterChannel::Latest), None, Some("0.109.0"));
        assert_eq!(latest.source, AdapterSource::Latest);
        assert_eq!(latest.version.as_deref(), Some("0.109.0"));
        assert_eq!(
            latest.spec.as_deref(),
            Some("@agentclientprotocol/claude-agent-acp@0.109.0")
        );
        assert_eq!(latest.verified_version, "0.84.0");

        let pin = adapter_status(k, &pinned("0.100.2"), None, Some("0.109.0"));
        assert_eq!(pin.source, AdapterSource::Pinned);
        assert_eq!(
            pin.spec.as_deref(),
            Some("@agentclientprotocol/claude-agent-acp@0.100.2")
        );

        let global = adapter_status(
            k,
            &channel(AdapterChannel::Global),
            Some("/home/u/.npm/bin/claude-agent-acp"),
            None,
        );
        assert_eq!(global.source, AdapterSource::Global);
        assert_eq!((&global.version, &global.spec), (&None, &None));
        assert_eq!(
            global.describe(),
            "/home/u/.npm/bin/claude-agent-acp",
            "the global executable is what runs"
        );

        let codex = adapter_status(
            AdapterKind::Codex,
            &channel(AdapterChannel::Latest),
            None,
            Some("2.8.0"),
        );
        assert_eq!(
            codex.spec.as_deref(),
            Some("@agentclientprotocol/codex-acp@2.8.0")
        );
    }

    #[test]
    fn a_choice_that_cannot_be_followed_runs_the_verified_version_and_says_why() {
        let k = AdapterKind::Codex;
        let no_latest = adapter_status(k, &channel(AdapterChannel::Latest), None, None);
        assert_eq!(no_latest.source, AdapterSource::Verified);
        assert_eq!(no_latest.version.as_deref(), Some("2.0.0"));
        assert!(
            no_latest
                .note
                .as_deref()
                .is_some_and(|n| n.contains("最新"))
        );

        let no_global = adapter_status(k, &channel(AdapterChannel::Global), None, Some("2.8.0"));
        assert_eq!(no_global.source, AdapterSource::Verified);
        assert!(
            no_global
                .note
                .as_deref()
                .is_some_and(|n| n.contains("codex-acp"))
        );

        // A stored version that is not valid (a hand-edited database) never reaches npx.
        let bad = adapter_status(k, &pinned("1.0.0 --yes"), None, None);
        assert_eq!(bad.source, AdapterSource::Verified);
        assert!(bad.note.is_some());
        let missing = adapter_status(
            k,
            &HarnessOptions {
                adapter: AdapterChannel::Pinned,
                ..HarnessOptions::default()
            },
            None,
            None,
        );
        assert_eq!(missing.source, AdapterSource::Verified);

        // A global install that is found is reported whichever channel is chosen.
        let found = adapter_status(
            k,
            &channel(AdapterChannel::Verified),
            Some("/bin/codex-acp"),
            None,
        );
        assert_eq!(found.global_path.as_deref(), Some("/bin/codex-acp"));
        assert_eq!(found.source, AdapterSource::Verified);
    }

    #[test]
    fn the_users_claude_runs_when_found_and_is_chosen() {
        let found = || {
            Some((
                "/home/u/.local/bin/claude".to_string(),
                PathSource::KnownDir,
            ))
        };
        let user = claude_executable(&HarnessOptions::default(), found());
        assert_eq!(user.using.as_deref(), Some("/home/u/.local/bin/claude"));
        assert_eq!(user.found, user.using);
        assert_eq!(user.path_source, PathSource::KnownDir);
        assert_eq!(user.note, None);

        let missing = claude_executable(&HarnessOptions::default(), None);
        assert_eq!(
            (missing.found.as_deref(), missing.using.as_deref()),
            (None, None)
        );
        assert_eq!(missing.path_source, PathSource::None);
        assert!(missing.note.as_deref().is_some_and(|n| n.contains("同梱")));

        let bundled = claude_executable(
            &HarnessOptions {
                claude_source: ClaudeSource::Bundled,
                ..HarnessOptions::default()
            },
            found(),
        );
        assert_eq!(bundled.using, None, "the bundled one is chosen");
        assert_eq!(bundled.found.as_deref(), Some("/home/u/.local/bin/claude"));
        assert_eq!(bundled.note, None);
    }

    #[test]
    fn the_launch_note_names_what_ran_and_how_to_go_back() {
        let k = AdapterKind::ClaudeAgent;
        let claude = |using: Option<&str>| ClaudeExecutable {
            found: using.map(str::to_string),
            path_source: PathSource::Path,
            using: using.map(str::to_string),
            note: None,
        };
        let verified = adapter_status(k, &HarnessOptions::default(), None, None);
        let plain = launch_note(k, &verified, Some(&claude(None)));
        assert!(plain.contains("claude-agent-acp@0.84.0") && plain.contains("検証済み"));
        assert!(plain.contains("同梱"));
        assert!(!plain.contains("戻す"), "nothing to go back to: {plain}");

        let latest = adapter_status(k, &channel(AdapterChannel::Latest), None, Some("0.109.0"));
        let note = launch_note(k, &latest, Some(&claude(Some("/u/claude"))));
        assert!(note.contains("claude-agent-acp@0.109.0 (最新)"), "{note}");
        assert!(note.contains("/u/claude"), "{note}");
        assert!(
            note.contains("「検証済み」(0.84.0)")
                && note.contains("「同梱」")
                && note.contains("戻す"),
            "{note}"
        );

        // The user's claude alone is a reason to say how to go back.
        let own = launch_note(k, &verified, Some(&claude(Some("/u/claude"))));
        assert!(own.contains("戻す") && own.contains("「同梱」"), "{own}");

        let codex = adapter_status(
            AdapterKind::Codex,
            &channel(AdapterChannel::Pinned),
            None,
            None,
        );
        // (no pinned version: the verified one ran)
        let codex_note = launch_note(AdapterKind::Codex, &codex, None);
        assert!(codex_note.contains("codex-acp@2.0.0") && !codex_note.contains("Claude Code"));
        let pinned_codex = adapter_status(AdapterKind::Codex, &pinned("2.5.0"), None, None);
        let pinned_note = launch_note(AdapterKind::Codex, &pinned_codex, None);
        assert!(
            pinned_note.contains("codex-acp@2.5.0 (指定)"),
            "{pinned_note}"
        );
        assert!(pinned_note.contains("「検証済み」(2.0.0)") && !pinned_note.contains("同梱"));
    }
}
