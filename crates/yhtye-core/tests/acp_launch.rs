//! Where and with which environment agent processes start (Stage 6b security
//! review): never in the project directory (an untrusted repository must not
//! configure the launcher, e.g. `npx` reading its `.npmrc`), and without the
//! dev bridge's token or the harness's `env_remove`. One test per binary: it changes the process environment.

use std::time::Duration;

use tokio::sync::mpsc;
use yhtye_core::acp::{HarnessConfig, SpawnOptions, spawn_agent};

#[tokio::test]
async fn agents_start_outside_the_project_without_yhtye_secrets() {
    let project = tempfile::tempdir().expect("project");
    let out = tempfile::tempdir().expect("out");
    let report = out.path().join("report");
    // SAFETY: the only test of this binary; nothing else reads the environment concurrently.
    unsafe {
        std::env::set_var("YHTYE_BRIDGE_TOKEN", "secret-token");
        std::env::set_var("YHTYE_TEST_DROPPED", "inherited");
        std::env::set_var("YHTYE_TEST_KEPT", "inherited");
    }
    let mut harness = HarnessConfig::plain(
        "sh",
        vec![
            "-c".into(),
            "{ pwd; echo \"token=${YHTYE_BRIDGE_TOKEN:-}\"; \
             echo \"dropped=${YHTYE_TEST_DROPPED:-} kept=${YHTYE_TEST_KEPT:-}\"; } > \"$0\"; exit 3"
                .into(),
            report.display().to_string(),
        ],
    );
    harness.startup_timeout = Duration::from_secs(10);
    // Stage 7c-2: e.g. an OPENCODE_CONFIG_DIR injected by an Orca terminal.
    harness.env_remove = vec!["YHTYE_TEST_DROPPED".into()];
    let (tx, _rx) = mpsc::unbounded_channel();
    let result = spawn_agent(&harness, project.path(), SpawnOptions::default(), tx).await;
    assert!(result.is_err(), "sh is not an ACP agent");
    let text = std::fs::read_to_string(&report).expect("the process ran");
    let mut lines = text.lines();
    let cwd = lines.next().expect("pwd");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    assert_eq!(
        std::fs::canonicalize(cwd).expect("cwd"),
        std::fs::canonicalize(&home).expect("home")
    );
    assert_ne!(
        std::fs::canonicalize(cwd).expect("cwd"),
        std::fs::canonicalize(project.path()).expect("project")
    );
    assert_eq!(lines.next(), Some("token="), "the bridge token leaked");
    assert_eq!(
        lines.next(),
        Some("dropped= kept=inherited"),
        "`env_remove` drops only the named inherited variables"
    );
}
