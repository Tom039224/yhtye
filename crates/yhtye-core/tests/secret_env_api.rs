//! The secret environment variable commands of the [`Core`] API (Stage 7e):
//! names are listed sorted and survive a restart, values never come back, and
//! problems are reported without leaking the value.

use std::sync::Arc;

use yhtye_core::api::{ApiCommand, ApiErrorCode, ApiResponse};
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_core::secrets::{MemoryBackend, SecretBackend, SecretValue};

const DUMMY: &str = "dummy-secret-value-123";

fn config(dir: &std::path::Path, backend: Arc<MemoryBackend>) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(dir, "haiku");
    cfg.usage = None;
    cfg.secret_backend = backend;
    cfg
}

fn names(r: ApiResponse) -> Vec<String> {
    match r {
        ApiResponse::SecretEnv { names } => names,
        other => panic!("unexpected {other:?}"),
    }
}

fn set(name: &str, value: &str) -> ApiCommand {
    ApiCommand::SetSecretEnv {
        name: name.into(),
        value: SecretValue::new(value),
    }
}

#[tokio::test]
async fn names_are_registered_listed_persisted_and_deleted_without_returning_values() {
    let dir = tempfile::tempdir().expect("dir");
    let backend = Arc::new(MemoryBackend::default());
    let core = Core::start(config(dir.path(), backend.clone()))
        .await
        .expect("start");
    assert!(names(core.command(ApiCommand::ListSecretEnv).await.expect("list")).is_empty());
    core.command(set("B_KEY", DUMMY)).await.expect("set");
    let after = core.command(set("A_KEY", DUMMY)).await.expect("set");
    assert_eq!(names(after.clone()), ["A_KEY", "B_KEY"]);
    let json = serde_json::to_string(&format!("{after:?}")).expect("json");
    assert!(!json.contains(DUMMY), "a response must not carry the value");
    assert_eq!(backend.get("A_KEY"), Ok(Some(DUMMY.into())));
    core.shutdown().await;
    drop(core);

    // The names come back after a restart; the values stay in the store.
    let core = Core::start(config(dir.path(), backend.clone()))
        .await
        .expect("restart");
    let listed = core.command(ApiCommand::ListSecretEnv).await.expect("list");
    assert_eq!(names(listed), ["A_KEY", "B_KEY"]);
    let left = core
        .command(ApiCommand::DeleteSecretEnv {
            name: "A_KEY".into(),
        })
        .await
        .expect("delete");
    assert_eq!(names(left), ["B_KEY"]);
    assert_eq!(backend.get("A_KEY"), Ok(None));
    core.shutdown().await;
}

#[tokio::test]
async fn bad_names_and_an_unavailable_store_are_rejected_without_leaking_the_value() {
    let dir = tempfile::tempdir().expect("dir");
    let backend = Arc::new(MemoryBackend::default());
    let core = Core::start(config(dir.path(), backend.clone()))
        .await
        .expect("start");
    let e = core
        .command(set("bad name", DUMMY))
        .await
        .expect_err("name");
    assert_eq!(e.code, ApiErrorCode::InvalidArgument);
    assert!(!e.message.contains(DUMMY));
    let e = core.command(set("OK_NAME", "")).await.expect_err("value");
    assert_eq!(e.code, ApiErrorCode::InvalidArgument);

    backend.fail_with("Secret Service is not running");
    let e = core
        .command(set("OK_NAME", DUMMY))
        .await
        .expect_err("store");
    assert_eq!(e.code, ApiErrorCode::Unavailable);
    assert!(e.message.contains("Secret Service is not running"), "{e}");
    assert!(!e.message.contains(DUMMY));
    let listed = core.command(ApiCommand::ListSecretEnv).await.expect("list");
    assert!(names(listed).is_empty(), "a failed store registers nothing");
    core.shutdown().await;
}
