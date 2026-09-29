use std::sync::Arc;

use super::*;

fn secrets() -> (Secrets, Arc<MemoryBackend>) {
    let backend = Arc::new(MemoryBackend::default());
    (Secrets::new(backend.clone(), []), backend)
}

#[test]
fn names_follow_the_environment_variable_charset() {
    for ok in ["A", "_x", "OPENROUTER_API_KEY_CODEX", "a1_B2"] {
        assert_eq!(validate_secret_name(ok), Ok(()), "{ok}");
    }
    for bad in [
        "",
        "1A",
        "A-B",
        "A B",
        "A=B",
        "日本",
        "YHTYE_BRIDGE_TOKEN",
        "PATH",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "NODE_OPTIONS",
    ] {
        assert!(validate_secret_name(bad).is_err(), "{bad}");
    }
    assert!(validate_secret_name(&"A".repeat(129)).is_err());
    assert!(validate_secret_value("").is_err());
    assert!(validate_secret_value("a\0b").is_err());
    assert!(validate_secret_value(&"x".repeat(MAX_SECRET_VALUE_BYTES + 1)).is_err());
    assert_eq!(validate_secret_value("sk-x y"), Ok(()));
}

#[tokio::test]
async fn set_registers_the_name_and_env_reads_the_value() {
    let (s, backend) = secrets();
    s.set("B_KEY", SecretValue::new("two")).await.expect("set");
    s.set("A_KEY", SecretValue::new("one")).await.expect("set");
    s.set("A_KEY", SecretValue::new("uno"))
        .await
        .expect("overwrite");
    assert_eq!(s.names(), ["A_KEY", "B_KEY"], "sorted, no duplicates");
    assert_eq!(backend.get("A_KEY"), Ok(Some("uno".into())));
    let env = s.env().await.expect("env");
    let vars: Vec<(&str, &str)> = env.iter().collect();
    assert_eq!(vars, [("A_KEY", "uno"), ("B_KEY", "two")]);
}

#[tokio::test]
async fn remove_forgets_the_name_and_the_value() {
    let (s, backend) = secrets();
    s.set("K", SecretValue::new("v")).await.expect("set");
    s.remove("K").await.expect("remove");
    s.remove("K").await.expect("removing again is fine");
    assert!(s.names().is_empty());
    assert_eq!(backend.get("K"), Ok(None));
    assert!(s.env().await.expect("env").is_empty());
}

#[tokio::test]
async fn invalid_input_is_rejected_before_the_store_is_touched() {
    let (s, backend) = secrets();
    backend.fail_with("must not be called");
    let e = s
        .set("1bad", SecretValue::new("v"))
        .await
        .expect_err("name");
    assert!(matches!(e, SecretError::Invalid(_)), "{e}");
    let e = s.set("OK", SecretValue::new("")).await.expect_err("value");
    assert!(matches!(e, SecretError::Invalid(_)), "{e}");
}

#[tokio::test]
async fn an_unavailable_store_is_an_error_and_registers_nothing() {
    let (s, backend) = secrets();
    backend.fail_with("no Secret Service");
    let e = s.set("K", SecretValue::new("v")).await.expect_err("set");
    assert_eq!(e, SecretError::Unavailable("no Secret Service".into()));
    assert!(e.to_string().contains("no Secret Service"));
    assert!(s.names().is_empty(), "a failed store must not register");
}

#[tokio::test]
async fn a_forbidden_name_from_a_hand_edited_row_is_not_injected() {
    let backend = Arc::new(MemoryBackend::default());
    backend.set("LD_PRELOAD", "/evil.so").expect("set");
    backend.set("OK_KEY", "v").expect("set");
    let s = Secrets::new(backend, ["LD_PRELOAD".to_string(), "OK_KEY".to_string()]);
    let env = s.env().await.expect("env");
    assert_eq!(env.names().collect::<Vec<_>>(), ["OK_KEY"]);
}

#[tokio::test]
async fn a_registered_name_without_a_value_is_skipped() {
    let backend = Arc::new(MemoryBackend::default());
    backend.set("HAS", "v").expect("set");
    let s = Secrets::new(backend, ["GONE".to_string(), "HAS".to_string()]);
    let env = s.env().await.expect("env");
    assert_eq!(env.names().collect::<Vec<_>>(), ["HAS"]);
}

#[test]
fn debug_output_never_shows_a_value() {
    let v = SecretValue::new("hunter2");
    assert!(!format!("{v:?}").contains("hunter2"));
    let env = SecretEnv::new(vec![("K".into(), v)]);
    let shown = format!("{env:?}");
    assert!(shown.contains('K') && !shown.contains("hunter2"), "{shown}");
}

/// Uses the real OS credential store (Secret Service). Run with
/// `cargo test -p yhtye-core secrets -- --ignored`.
#[tokio::test]
#[ignore = "needs a running credential store (gnome-keyring / KWallet)"]
async fn the_os_keyring_round_trips_a_dummy_secret() {
    let backend = Arc::new(KeyringBackend::with_service("yhtye-test"));
    let s = Secrets::new(backend.clone(), []);
    let name = "YHTYE_TEST_SECRET_ROUNDTRIP";
    s.set(name, SecretValue::new("dummy-value"))
        .await
        .expect("store in the keyring");
    assert_eq!(backend.get(name), Ok(Some("dummy-value".into())));
    s.remove(name).await.expect("remove");
    assert_eq!(backend.get(name), Ok(None));
}
