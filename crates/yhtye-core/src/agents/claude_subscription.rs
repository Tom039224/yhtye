//! The name of the Claude subscription the user's Claude Code is logged in to
//! (`Claude Max`, `Claude Pro`, ...), for the harness label `Claude Code
//! (Claude Max)`. Read from the `claudeAiOauth` entry of Claude Code's
//! `.credentials.json`: only `subscriptionType` and `rateLimitTier` are
//! looked at, the tokens beside them are never kept or logged, and the file
//! is never written. Where the file does not exist (macOS keeps the login in
//! the Keychain) or says nothing usable, the label is `サブスク`.

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;

use serde::Deserialize;

/// Claude Code's login file inside its config directory.
const CREDENTIALS_FILE: &str = ".credentials.json";

/// The most that is read of the file (it holds a few tokens).
const MAX_CREDENTIALS_BYTES: u64 = 256 * 1024;

/// What the label says when the subscription is not known.
const UNKNOWN_SUBSCRIPTION: &str = "サブスク";

/// Claude Code's config directory: `CLAUDE_CONFIG_DIR` when set and non-empty,
/// else `~/.claude`.
fn claude_config_dir(env: &(impl Fn(&str) -> Option<OsString> + ?Sized)) -> Option<PathBuf> {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    non_empty("CLAUDE_CONFIG_DIR")
        .or_else(|| non_empty("HOME").map(|home| home.join(".claude")))
        .filter(|p| p.is_absolute())
}

#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OauthEntry>,
}

/// The two fields of the login that name the plan (the token fields are not
/// even deserialized).
#[derive(Deserialize)]
struct OauthEntry {
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

/// The plan name of the `.credentials.json` text: `Claude Max 20x`, `Claude
/// Pro`, `Claude Team`, `Claude Enterprise`; `None` when the text is not that
/// JSON or names no known plan.
#[must_use]
pub fn subscription_name(credentials_json: &str) -> Option<String> {
    let entry = serde_json::from_str::<Credentials>(credentials_json)
        .ok()?
        .oauth?;
    let kind = entry.subscription_type?.trim().to_ascii_lowercase();
    let plan = match kind.as_str() {
        "max" => "Claude Max",
        "pro" => "Claude Pro",
        "team" => "Claude Team",
        "enterprise" => "Claude Enterprise",
        _ => return None,
    };
    match kind.as_str() {
        "max" => match entry.rate_limit_tier.as_deref().and_then(max_multiplier) {
            Some(n) => Some(format!("{plan} {n}x")),
            None => Some(plan.to_string()),
        },
        _ => Some(plan.to_string()),
    }
}

/// The `20` of a rate limit tier such as `default_claude_max_20x`.
fn max_multiplier(tier: &str) -> Option<u32> {
    let rest = &tier[tier.find("max_")? + "max_".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    rest[digits.len()..]
        .starts_with('x')
        .then(|| digits.parse().ok())
        .flatten()
}

/// The name of the subscription Claude Code is logged in to, read from
/// `.credentials.json` in the config directory `env` names (`CLAUDE_CONFIG_DIR`,
/// else `HOME`/.claude). `Claude Code` is not part of it.
#[must_use]
pub fn subscription_of(env: &(impl Fn(&str) -> Option<OsString> + ?Sized)) -> Option<String> {
    let path = claude_config_dir(env)?.join(CREDENTIALS_FILE);
    let mut text = String::new();
    let read = std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_CREDENTIALS_BYTES).read_to_string(&mut text));
    if let Err(e) = read {
        // The text is not logged: it holds tokens.
        tracing::debug!("no Claude subscription login at {}: {e}", path.display());
        return None;
    }
    subscription_name(&text)
}

/// The label of the subscription Claude Code: `Claude Code (Claude Max)`, or
/// `Claude Code (サブスク)` when the plan is not known.
#[must_use]
pub fn claude_code_label(env: &(impl Fn(&str) -> Option<OsString> + ?Sized)) -> String {
    let plan = subscription_of(env).unwrap_or_else(|| UNKNOWN_SUBSCRIPTION.to_string());
    format!("Claude Code ({plan})")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn login(subscription: &str, tier: Option<&str>) -> String {
        let tier = tier.map_or(String::new(), |t| format!(r#","rateLimitTier":"{t}""#));
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"sk-ant-oat01-SECRET","refreshToken":"sk-ant-ort01-SECRET","expiresAt":1,"scopes":["user:inference"],"subscriptionType":"{subscription}"{tier}}}}}"#
        )
    }

    #[test]
    fn each_plan_has_its_name_and_a_max_plan_its_multiplier() {
        let cases = [
            ("max", Some("default_claude_max_20x"), "Claude Max 20x"),
            ("max", Some("default_claude_max_5x"), "Claude Max 5x"),
            ("max", Some("default_claude_ai"), "Claude Max"),
            ("max", Some("default_claude_max_x"), "Claude Max"),
            ("max", Some("default_claude_max_20"), "Claude Max"),
            ("max", None, "Claude Max"),
            ("pro", Some("default_claude_ai"), "Claude Pro"),
            ("pro", Some("default_claude_max_5x"), "Claude Pro"),
            ("team", None, "Claude Team"),
            ("enterprise", None, "Claude Enterprise"),
            ("Max", Some("default_claude_max_20x"), "Claude Max 20x"),
            (" PRO ", None, "Claude Pro"),
        ];
        for (kind, tier, want) in cases {
            assert_eq!(
                subscription_name(&login(kind, tier)).as_deref(),
                Some(want),
                "{kind} {tier:?}"
            );
        }
    }

    #[test]
    fn an_unknown_or_missing_plan_has_no_name() {
        for text in [
            login("free", None),
            login("", None),
            r#"{"claudeAiOauth":{"accessToken":"x"}}"#.to_string(),
            r#"{"claudeAiOauth":{"subscriptionType":7}}"#.to_string(),
            r#"{"claudeAiOauth":null}"#.to_string(),
            r#"{"mcpOAuth":{}}"#.to_string(),
            "{}".to_string(),
            "[]".to_string(),
            "not json".to_string(),
            "{\"claudeAiOauth\":".to_string(),
            String::new(),
        ] {
            assert_eq!(subscription_name(&text), None, "{text}");
        }
    }

    #[test]
    fn the_label_is_read_from_the_credentials_file_under_home() {
        let home = tempfile::tempdir().expect("home");
        let dir = home.path().join(".claude");
        std::fs::create_dir(&dir).expect("mkdir");
        std::fs::write(
            dir.join(".credentials.json"),
            login("max", Some("default_claude_max_20x")),
        )
        .expect("write");
        let home_str = home.path().to_str().expect("utf-8");
        assert_eq!(
            claude_code_label(&env(&[("HOME", home_str)])),
            "Claude Code (Claude Max 20x)"
        );
    }

    #[test]
    fn claude_config_dir_moves_the_credentials_file() {
        let home = tempfile::tempdir().expect("home");
        let config = tempfile::tempdir().expect("config");
        std::fs::write(config.path().join(".credentials.json"), login("team", None))
            .expect("write");
        let vars = [
            ("HOME", home.path().to_str().expect("utf-8")),
            ("CLAUDE_CONFIG_DIR", config.path().to_str().expect("utf-8")),
        ];
        assert_eq!(claude_code_label(&env(&vars)), "Claude Code (Claude Team)");
    }

    #[test]
    fn the_label_falls_back_when_the_login_cannot_be_read() {
        let fallback = "Claude Code (サブスク)";
        // No home, a relative home, no file.
        assert_eq!(claude_code_label(&env(&[])), fallback);
        assert_eq!(claude_code_label(&env(&[("HOME", "relative")])), fallback);
        let home = tempfile::tempdir().expect("home");
        let home_str = home.path().to_str().expect("utf-8");
        assert_eq!(claude_code_label(&env(&[("HOME", home_str)])), fallback);

        // A broken file, a directory in its place, an unknown plan.
        let dir = home.path().join(".claude");
        std::fs::create_dir(&dir).expect("mkdir");
        let file = dir.join(".credentials.json");
        std::fs::write(&file, "{ not json").expect("write");
        assert_eq!(claude_code_label(&env(&[("HOME", home_str)])), fallback);
        std::fs::remove_file(&file).expect("rm");
        std::fs::create_dir(&file).expect("mkdir");
        assert_eq!(claude_code_label(&env(&[("HOME", home_str)])), fallback);
        std::fs::remove_dir(&file).expect("rmdir");
        std::fs::write(&file, login("mystery", None)).expect("write");
        assert_eq!(claude_code_label(&env(&[("HOME", home_str)])), fallback);
    }

    #[test]
    fn the_label_never_contains_the_tokens() {
        let home = tempfile::tempdir().expect("home");
        let dir = home.path().join(".claude");
        std::fs::create_dir(&dir).expect("mkdir");
        std::fs::write(dir.join(".credentials.json"), login("pro", None)).expect("write");
        let label = claude_code_label(&env(&[("HOME", home.path().to_str().expect("utf-8"))]));
        assert!(
            !label.contains("SECRET") && !label.contains("sk-ant"),
            "{label}"
        );
    }
}
