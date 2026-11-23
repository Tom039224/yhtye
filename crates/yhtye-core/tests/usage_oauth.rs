//! [`UsageService`] with the OAuth usage endpoint: it answers without starting
//! the agent, falls back to `/usage` in the agent when the endpoint gives
//! nothing, and does not fall back when it is rate limited. The endpoint is a
//! local HTTP server, the agent a fake one (Unix: it is a script-driven binary).

#![cfg(unix)]

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fake_harness;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::usage::{OAuthUsage, UsageError, UsageReport, UsageService, UsageWindowKind};

const OAUTH_BODY: &str = r#"{"five_hour": {"utilization": 12.0, "resets_at": "2026-11-23T05:00:00Z"}, "seven_day": null}"#;

/// What the endpoint answers to every request.
struct Answer {
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: &'static str,
}

fn answer(status: u16, body: &'static str) -> Answer {
    Answer {
        status,
        headers: vec![],
        body,
    }
}

/// A local endpoint; counts the requests it got.
async fn endpoint(answer: Answer) -> (String, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!(
        "http://{}/api/oauth/usage",
        listener.local_addr().expect("addr")
    );
    let calls = Arc::new(Mutex::new(0));
    let counted = calls.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8192];
            let _ = socket.read(&mut buf).await;
            *counted.lock().expect("calls") += 1;
            let mut head = format!(
                "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                answer.status,
                answer.body.len()
            );
            for (k, v) in &answer.headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(answer.body.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (url, calls)
}

fn calls(counter: &Arc<Mutex<usize>>) -> usize {
    *counter.lock().expect("calls")
}

/// A config dir with a login (`token`), or an empty one.
fn config_dir(login: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    if login {
        std::fs::write(
            dir.path().join(".credentials.json"),
            r#"{"claudeAiOauth": {"accessToken": "t", "subscriptionType": "max"}}"#,
        )
        .expect("write credentials");
    }
    dir
}

fn oauth(url: &str, config: &Path) -> OAuthUsage {
    OAuthUsage::new()
        .with_url(url)
        .with_config_dir(config)
        .with_timeout(Duration::from_secs(5))
}

/// An agent whose `/usage` says 55 % (what the fallback reads).
fn acp_harness() -> HarnessConfig {
    let text = "> Claude max subscription usage\n\
                **5-hour limit** — **55%** · Resets Sep 22, 10:50 PM UTC\n";
    fake_harness(json!({"turns": [{"match": "/usage", "actions": [{"message": text}]}]}))
}

/// An agent that cannot start: reaching it fails the read.
fn missing_agent() -> HarnessConfig {
    HarnessConfig::plain("/nonexistent/yhtye-usage-agent".to_string(), vec![])
}

fn service(harness: HarnessConfig, oauth: OAuthUsage) -> (UsageService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = UsageService::new(Some(harness), dir.path().join("probe")).with_oauth(oauth);
    (service, dir)
}

fn five_hour(report: &UsageReport) -> f64 {
    report
        .window(UsageWindowKind::FiveHour)
        .expect("5h")
        .percent
}

#[tokio::test]
async fn an_oauth_answer_is_used_without_starting_the_agent_and_is_cached() {
    let config = config_dir(true);
    let (url, hits) = endpoint(answer(200, OAUTH_BODY)).await;
    let (service, _dir) = service(missing_agent(), oauth(&url, config.path()));
    let report = service.get(false).await.expect("read through oauth");
    assert!((five_hour(&report) - 12.0).abs() < f64::EPSILON);
    assert_eq!(report.plan.as_deref(), Some("max"));
    // Within the cache time, neither the endpoint nor the agent is asked again.
    assert_eq!(service.get(false).await.expect("cached"), report);
    assert_eq!(calls(&hits), 1);
    service.close().await;
}

#[tokio::test]
async fn it_falls_back_to_the_agent_when_the_endpoint_gives_nothing() {
    for (label, login, reply) in [
        ("no login", false, answer(200, OAUTH_BODY)),
        ("stale token", true, answer(401, "{}")),
        ("forbidden", true, answer(403, "{}")),
        ("server error", true, answer(500, "{}")),
        ("bad json", true, answer(200, "nope")),
        (
            "no window",
            true,
            answer(200, r#"{"five_hour": null, "seven_day": null}"#),
        ),
    ] {
        let config = config_dir(login);
        let (url, _) = endpoint(reply).await;
        let (service, _dir) = service(acp_harness(), oauth(&url, config.path()));
        let report = service
            .get(false)
            .await
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!((five_hour(&report) - 55.0).abs() < f64::EPSILON, "{label}");
        service.close().await;
    }
}

#[tokio::test]
async fn an_unreachable_endpoint_falls_back_to_the_agent() {
    let config = config_dir(true);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}/", listener.local_addr().expect("addr"));
    drop(listener);
    let (service, _dir) = service(acp_harness(), oauth(&url, config.path()));
    let report = service.get(false).await.expect("fallback");
    assert!((five_hour(&report) - 55.0).abs() < f64::EPSILON);
    service.close().await;
}

#[tokio::test]
async fn a_429_does_not_fall_back_and_the_endpoint_is_left_alone() {
    let config = config_dir(true);
    let reply = Answer {
        status: 429,
        headers: vec![("retry-after", "120")],
        body: "{}",
    };
    let (url, hits) = endpoint(reply).await;
    // The agent would read 55 %: getting it would mean the fallback ran.
    let (service, _dir) = service(acp_harness(), oauth(&url, config.path()));
    for refresh in [false, true, true] {
        let Err(UsageError::RateLimited { retry_after }) = service.get(refresh).await else {
            panic!("expected RateLimited");
        };
        let wait = retry_after.expect("the wait is known");
        assert!(
            wait <= Duration::from_secs(120) && wait > Duration::from_secs(100),
            "{wait:?}"
        );
    }
    assert_eq!(calls(&hits), 1, "not asked again after the 429");
    service.close().await;
}

#[tokio::test]
async fn oauth_alone_serves_usage_when_no_agent_is_configured() {
    let config = config_dir(true);
    let (url, _) = endpoint(answer(200, OAUTH_BODY)).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let service =
        UsageService::new(None, dir.path().join("probe")).with_oauth(oauth(&url, config.path()));
    assert!((five_hour(&service.get(false).await.expect("read")) - 12.0).abs() < f64::EPSILON);
    // Without the login the answer is the missing agent's.
    let no_login = config_dir(false);
    let service =
        UsageService::new(None, dir.path().join("probe")).with_oauth(oauth(&url, no_login.path()));
    assert_eq!(service.get(false).await, Err(UsageError::NotConfigured));
    let bare = UsageService::new(None, dir.path().join("probe"));
    assert_eq!(bare.get(false).await, Err(UsageError::NotConfigured));
}
