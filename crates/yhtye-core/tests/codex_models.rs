//! Where the Codex harness's models come from (Stage 7e): OpenRouter's public
//! list when the user's Codex config uses the `openrouter` provider (fetched
//! without credentials, with each model's efforts), the adapter's own list
//! (over ACP) otherwise.

mod common;

use std::sync::Arc;

use common::fake_harness;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::agents::{HarnessPreset, ModelService, ModelSource};
use yhtye_core::secrets::Secrets;

/// Serves `body` for every request and records the raw requests.
async fn serve(body: String) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!(
        "http://{}/api/v1/models",
        listener.local_addr().expect("addr")
    );
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            log.lock()
                .expect("log")
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
        }
    });
    (url, seen)
}

fn codex_home(provider: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("home");
    std::fs::write(
        dir.path().join("config.toml"),
        format!("model = \"x/y\"\nmodel_provider = \"{provider}\"\n"),
    )
    .expect("config");
    dir
}

fn service(cwd: &std::path::Path, home: &std::path::Path, url: &str) -> ModelService {
    let home = home.display().to_string();
    ModelService::new(cwd.join("probe"), Arc::new(Secrets::none()))
        .with_env(move |k| (k == "CODEX_HOME").then(|| home.clone().into()))
        .with_openrouter_url(url)
}

/// A Codex-like preset whose adapter is the fake agent (it offers `m-acp`).
fn preset_with_fake_adapter() -> HarnessPreset {
    let script = json!({"models": ["m-acp"], "turns": []});
    let h: HarnessConfig = fake_harness(script);
    let mut p = HarnessPreset::fixed("codex", h.clone(), h.clone(), h);
    p.model_source = ModelSource::Codex;
    p
}

#[tokio::test]
async fn the_openrouter_provider_lists_openrouter_models_with_their_efforts_and_no_credentials() {
    let body = json!({"data": [
        {"id": "a/effort", "name": "A", "supported_parameters": ["tools"],
         "reasoning": {"supported_efforts": ["high", "low"]}},
        {"id": "b/plain:free", "name": "B", "supported_parameters": ["tools"]},
        {"id": "c/no-tools", "supported_parameters": []}
    ]})
    .to_string();
    let (url, seen) = serve(body).await;
    let home = codex_home("openrouter");
    let cwd = tempfile::tempdir().expect("cwd");
    let svc = service(cwd.path(), home.path(), &url);
    let preset = preset_with_fake_adapter();

    let listed = svc.get(&preset, false).await.expect("models");
    let ids: Vec<&str> = listed.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(ids, ["a/effort", "b/plain:free"], "ACP was not asked");
    assert_eq!(listed.harness, "codex");

    let efforts = svc.get_efforts(&preset, "a/effort").await.expect("efforts");
    let values: Vec<&str> = efforts.iter().map(|e| e.value.as_str()).collect();
    assert_eq!(values, ["high", "low"]);
    assert!(
        svc.get_efforts(&preset, "b/plain:free")
            .await
            .expect("none")
            .is_empty()
    );

    let requests = seen.lock().expect("log").clone();
    assert_eq!(requests.len(), 1, "one download, then cached: {requests:?}");
    let lower = requests[0].to_lowercase();
    assert!(lower.starts_with("get /api/v1/models"), "{lower}");
    assert!(
        !lower.contains("authorization") && !lower.contains("api-key") && !lower.contains("bearer"),
        "the public endpoint must be asked without credentials: {lower}"
    );
}

#[tokio::test]
async fn another_provider_lists_what_the_adapter_offers() {
    let (url, seen) = serve("{\"data\": []}".into()).await;
    let home = codex_home("acme");
    let cwd = tempfile::tempdir().expect("cwd");
    let svc = service(cwd.path(), home.path(), &url);
    let listed = svc
        .get(&preset_with_fake_adapter(), false)
        .await
        .expect("models");
    let ids: Vec<&str> = listed.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(ids, ["m-acp"]);
    assert!(
        seen.lock().expect("log").is_empty(),
        "OpenRouter was not asked"
    );
}

#[tokio::test]
async fn a_failing_download_is_an_error_not_an_empty_list() {
    let home = codex_home("openrouter");
    let cwd = tempfile::tempdir().expect("cwd");
    // Nothing listens here.
    let svc = service(cwd.path(), home.path(), "http://127.0.0.1:1/api/v1/models");
    let err = svc
        .get(&preset_with_fake_adapter(), false)
        .await
        .expect_err("fails");
    assert!(err.contains("127.0.0.1:1"), "{err}");
}
