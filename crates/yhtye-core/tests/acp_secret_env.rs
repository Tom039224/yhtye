//! Secret environment variables reach the agent process (Stage 7e): those
//! registered in [`Secrets`] are injected by `spawn_agent_with_secrets`, the
//! harness's own `env` wins on a clash, and a broken credential store fails
//! the spawn without starting anything.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::{AgentError, HarnessConfig, SpawnOptions};
use yhtye_core::secrets::{MemoryBackend, SecretValue, Secrets, spawn_agent_with_secrets};

fn reporting_harness(report: &std::path::Path) -> HarnessConfig {
    let mut harness = HarnessConfig::plain(
        "sh",
        vec![
            "-c".into(),
            "echo \"a=${YHTYE_SECRET_A:-} b=${YHTYE_SECRET_B:-}\" > \"$0\"; exit 3".into(),
            report.display().to_string(),
        ],
    );
    harness.startup_timeout = Duration::from_secs(10);
    harness
}

#[tokio::test]
async fn registered_secrets_are_in_the_agent_environment_and_harness_env_wins() {
    let out = tempfile::tempdir().expect("out");
    let report = out.path().join("report");
    let secrets = Secrets::new(Arc::new(MemoryBackend::default()), []);
    secrets
        .set("YHTYE_SECRET_A", SecretValue::new("from-keyring"))
        .await
        .expect("set");
    secrets
        .set("YHTYE_SECRET_B", SecretValue::new("from-keyring"))
        .await
        .expect("set");
    let mut harness = reporting_harness(&report);
    harness
        .env
        .insert("YHTYE_SECRET_B".into(), "from-harness".into());
    let (tx, _rx) = mpsc::unbounded_channel();
    let cwd = tempfile::tempdir().expect("cwd");
    let result =
        spawn_agent_with_secrets(&secrets, &harness, cwd.path(), SpawnOptions::default(), tx).await;
    assert!(result.is_err(), "sh is not an ACP agent");
    let text = std::fs::read_to_string(&report).expect("the process ran");
    assert_eq!(text.trim(), "a=from-keyring b=from-harness");
}

#[tokio::test]
async fn an_unreadable_credential_store_fails_the_spawn_before_starting_the_agent() {
    let out = tempfile::tempdir().expect("out");
    let report = out.path().join("report");
    let backend = Arc::new(MemoryBackend::default());
    let secrets = Secrets::new(backend.clone(), ["YHTYE_SECRET_A".to_string()]);
    backend.fail_with("Secret Service is not running");
    let harness = reporting_harness(&report);
    let (tx, _rx) = mpsc::unbounded_channel();
    let cwd = tempfile::tempdir().expect("cwd");
    let err = spawn_agent_with_secrets(&secrets, &harness, cwd.path(), SpawnOptions::default(), tx)
        .await
        .err()
        .expect("fails");
    let message = err.to_string();
    assert!(
        message.contains("secret environment variables")
            && message.contains("Secret Service is not running"),
        "{message}"
    );
    assert!(!report.exists(), "the agent must not have been started");
}

/// `base` (a Claude Code config with its real `env_remove` and
/// `secret_bindings`, or a plain one) running a shell that writes what it was
/// given to `report`, one `name=value` per line, instead of Claude Code.
fn claude_reporting_harness(base: HarnessConfig, report: &std::path::Path) -> HarnessConfig {
    HarnessConfig {
        command: "sh".into(),
        args: vec![
            "-c".into(),
            r#"printf 'key=%s\nsrc=%s\ntoken=%s\noauth=%s\nother=%s\n' \
                "${ANTHROPIC_API_KEY-}" "${YHTYE_ANTHROPIC_API_KEY-}" \
                "${ANTHROPIC_AUTH_TOKEN-}" "${CLAUDE_CODE_OAUTH_TOKEN-}" \
                "${OTHER_KEY-}" > "$0"; exit 3"#
                .into(),
            report.display().to_string(),
        ],
        startup_timeout: Duration::from_secs(10),
        ..base
    }
}

/// What a process was given, as written by [`claude_reporting_harness`].
/// The machine running the tests may have real credentials in its own
/// environment, so a value is only ever *compared* with an expected one and
/// never shown: a failing assertion says `unset` / `expected` / `other`.
struct Seen(std::collections::HashMap<String, String>);

impl Seen {
    fn state(&self, name: &str, expected: &str) -> &'static str {
        match self.0.get(name).map(String::as_str) {
            None | Some("") => "unset",
            Some(value) if value == expected => "expected",
            Some(_) => "other",
        }
    }

    /// [`Seen::state`] of each `(name, expected value)`.
    fn states<const N: usize>(&self, checks: [(&str, &str); N]) -> [&'static str; N] {
        checks.map(|(name, expected)| self.state(name, expected))
    }
}

async fn registered(names_and_values: &[(&str, &str)]) -> Secrets {
    let secrets = Secrets::new(Arc::new(MemoryBackend::default()), []);
    for (name, value) in names_and_values {
        secrets
            .set(name, SecretValue::new(*value))
            .await
            .expect("set");
    }
    secrets
}

async fn run(secrets: &Secrets, harness: &HarnessConfig) -> Result<(), AgentError> {
    let (tx, _rx) = mpsc::unbounded_channel();
    let cwd = tempfile::tempdir().expect("cwd");
    spawn_agent_with_secrets(secrets, harness, cwd.path(), SpawnOptions::default(), tx)
        .await
        .map(|_| ())
}

/// Starts `base` (as a reporting shell) with `secrets` and reads what it got.
async fn seen_by(secrets: &Secrets, base: HarnessConfig) -> Seen {
    let out = tempfile::tempdir().expect("out");
    let report = out.path().join("report");
    let harness = claude_reporting_harness(base, &report);
    assert!(
        run(secrets, &harness).await.is_err(),
        "sh is not an ACP agent"
    );
    let text = std::fs::read_to_string(&report).expect("the process ran");
    Seen(
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    )
}

// The tests below hold whatever the machine's own environment has: a real
// `ANTHROPIC_API_KEY` / `YHTYE_ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` /
// `CLAUDE_CODE_OAUTH_TOKEN` must change no result (and appear in no message).

#[tokio::test]
async fn claude_code_api_alone_gets_the_api_key_as_anthropic_api_key() {
    let secrets = registered(&[
        ("YHTYE_ANTHROPIC_API_KEY", "fake-key"),
        ("OTHER_KEY", "other"),
        // Credentials that would be used before the API key.
        ("ANTHROPIC_AUTH_TOKEN", "fake-token"),
        ("CLAUDE_CODE_OAUTH_TOKEN", "fake-oauth"),
    ])
    .await;
    let seen = seen_by(&secrets, HarnessConfig::claude_code_api("haiku")).await;
    assert_eq!(
        seen.states([
            ("key", "fake-key"),
            ("src", "fake-key"),
            ("token", "fake-token"),
            ("oauth", "fake-oauth"),
            ("other", "other"),
        ]),
        ["expected", "unset", "unset", "unset", "expected"],
        "[key, src, token, oauth, other]: the key under the name Claude Code reads, \
         not under its own; the other credentials removed"
    );
}

#[tokio::test]
async fn the_api_key_wins_over_a_general_secret_of_the_same_name() {
    let secrets = registered(&[
        ("YHTYE_ANTHROPIC_API_KEY", "bound"),
        ("ANTHROPIC_API_KEY", "general"),
    ])
    .await;
    let seen = seen_by(&secrets, HarnessConfig::claude_code_api("haiku")).await;
    assert_eq!(seen.state("key", "bound"), "expected");
}

#[tokio::test]
async fn the_subscription_claude_code_gets_no_api_key_from_any_secret() {
    let secrets = registered(&[
        ("YHTYE_ANTHROPIC_API_KEY", "fake-key"),
        // A general secret under the name Claude Code reads would bill the API.
        ("ANTHROPIC_API_KEY", "fake-general"),
        ("ANTHROPIC_AUTH_TOKEN", "fake-token"),
        ("OTHER_KEY", "other"),
    ])
    .await;
    for base in [
        HarnessConfig::claude_code("haiku"),
        HarnessConfig::claude_code_usage_probe(),
    ] {
        let seen = seen_by(&secrets, base).await;
        assert_eq!(
            seen.states([
                ("key", "fake-general"),
                ("src", "fake-key"),
                ("token", "fake-token"),
                ("other", "other"),
            ]),
            ["unset", "unset", "unset", "expected"],
            "[key, src, token, other]: neither the API key secret nor the credentials \
             of the API are in its environment"
        );
    }
}

#[tokio::test]
async fn another_harness_does_not_get_the_api_key_secret_either() {
    let secrets = registered(&[("YHTYE_ANTHROPIC_API_KEY", "fake-key")]).await;
    let seen = seen_by(&secrets, HarnessConfig::plain("sh", Vec::new())).await;
    assert_eq!(
        seen.state("src", "fake-key"),
        "unset",
        "a harness that does not bind it never sees it"
    );
}

/// What the parent environment of [`a_harness_only_secret_in_the_inherited_environment_reaches_no_harness`]
/// holds, for every name that matters.
const PARENT_VALUE: &str = "parent-env-value";
/// Set only in the child process started by that test.
const CHILD_MARKER: &str = "YHTYE_TEST_PARENT_ENV_CHILD";

/// Starts this test binary again with the credentials in *its* environment
/// (the environment Yhtye would have inherited from where it was started), so
/// the test process's own environment is never changed. The checks are in
/// [`parent_environment_child`].
#[test]
fn a_harness_only_secret_in_the_inherited_environment_reaches_no_harness() {
    let exe = std::env::current_exe().expect("the test binary");
    let output = std::process::Command::new(exe)
        .args([
            "--ignored",
            "--exact",
            "parent_environment_child",
            "--test-threads=1",
        ])
        .env(CHILD_MARKER, "1")
        .env("YHTYE_ANTHROPIC_API_KEY", PARENT_VALUE)
        .env("ANTHROPIC_API_KEY", PARENT_VALUE)
        .env("ANTHROPIC_AUTH_TOKEN", PARENT_VALUE)
        .env("CLAUDE_CODE_OAUTH_TOKEN", PARENT_VALUE)
        .output()
        .expect("start the test binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "the child failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
#[ignore = "run by a_harness_only_secret_in_the_inherited_environment_reaches_no_harness"]
async fn parent_environment_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        eprintln!("not started by its parent test: nothing to check");
        return;
    }
    let p = PARENT_VALUE;
    // The API key is not registered: it is only in the inherited environment,
    // and no harness gets it under its own name.
    let none = Secrets::none();
    for base in [
        HarnessConfig::claude_code("haiku"),
        HarnessConfig::claude_code_usage_probe(),
        HarnessConfig::plain("sh", Vec::new()),
    ] {
        let seen = seen_by(&none, base).await;
        assert_eq!(seen.state("src", p), "unset");
    }

    // The subscription Claude Code loses the API credentials it inherited.
    for base in [
        HarnessConfig::claude_code("haiku"),
        HarnessConfig::claude_code_usage_probe(),
    ] {
        let seen = seen_by(&none, base).await;
        assert_eq!(
            seen.states([("key", p), ("token", p)]),
            ["unset", "unset"],
            "[key, token]"
        );
    }

    // With the key registered: the API harness gets it as `ANTHROPIC_API_KEY`
    // (not what it inherited), loses the credentials that rank before it, and
    // nothing gets `YHTYE_ANTHROPIC_API_KEY` itself.
    let secrets = registered(&[("YHTYE_ANTHROPIC_API_KEY", "bound")]).await;
    let seen = seen_by(&secrets, HarnessConfig::claude_code_api("haiku")).await;
    assert_eq!(
        seen.states([("key", "bound"), ("src", p), ("token", p), ("oauth", p)]),
        ["expected", "unset", "unset", "unset"],
        "[key, src, token, oauth]"
    );
    for base in [
        HarnessConfig::claude_code("haiku"),
        HarnessConfig::plain("sh", Vec::new()),
    ] {
        let seen = seen_by(&secrets, base).await;
        assert_eq!(
            seen.states([("src", p), ("src", "bound")]),
            ["unset", "unset"]
        );
    }
}

#[tokio::test]
async fn claude_code_api_does_not_start_without_its_key_and_says_so() {
    let expected = "Claude Code (API) には秘密の環境変数 YHTYE_ANTHROPIC_API_KEY の登録が必要です";
    let out = tempfile::tempdir().expect("out");
    let report = out.path().join("report");
    let harness = claude_reporting_harness(HarnessConfig::claude_code_api("haiku"), &report);

    // Not registered at all (another secret is, and is no substitute).
    let secrets = registered(&[("ANTHROPIC_API_KEY", "general")]).await;
    let err = run(&secrets, &harness).await.expect_err("fails");
    assert!(matches!(err, AgentError::Setup { .. }), "{err:?}");
    assert!(err.to_string().contains(expected), "{err}");
    assert!(!err.to_string().contains("general"), "{err}");
    assert!(!report.exists(), "the agent must not have been started");

    // Registered, but the credential store has no value for it.
    let secrets = Secrets::new(
        Arc::new(MemoryBackend::default()),
        ["YHTYE_ANTHROPIC_API_KEY".to_string()],
    );
    let err = run(&secrets, &harness).await.expect_err("fails");
    assert!(err.to_string().contains(expected), "{err}");
    assert!(!report.exists(), "the agent must not have been started");
}

#[tokio::test]
async fn listing_the_models_of_claude_code_api_without_a_key_fails_with_the_hint() {
    use yhtye_core::agents::{HarnessPreset, probe_models};
    let dir = tempfile::tempdir().expect("cwd");
    let preset = HarnessPreset::claude_code_api("haiku").with_command("/nonexistent/npx");
    let error = probe_models(
        &Secrets::none(),
        &preset,
        dir.path(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect_err("no key, no listing");
    assert!(
        error.contains(
            "Claude Code (API) には秘密の環境変数 YHTYE_ANTHROPIC_API_KEY の登録が必要です"
        ),
        "{error}"
    );
}
