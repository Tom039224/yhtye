//! Secret environment variables reach the agent process (Stage 7e): those
//! registered in [`Secrets`] are injected by `spawn_agent_with_secrets`, the
//! harness's own `env` wins on a clash, and a broken credential store fails
//! the spawn without starting anything.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::{HarnessConfig, SpawnOptions};
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
