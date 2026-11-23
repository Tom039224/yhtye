//! Reads the subscription usage from Anthropic's OAuth usage endpoint, with the
//! access token Claude Code keeps in `<config dir>/.credentials.json` (the way
//! Orca does it). It answers in one request what the `/usage` command of the
//! ACP adapter needs an agent process for, and without the adapter's 5-second
//! cut-off: [`super::probe`] uses it first and falls back to `/usage`.
//!
//! The token is only read and sent to the endpoint: it is never refreshed
//! (a rejected token is the fallback's business) and never logged.
//!
//! TODO: macOS keeps the login in the Keychain (`Claude Code-credentials`),
//! not in the file; reading it is not supported yet (the fallback runs there).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::parse::days_from_civil;
use super::{UsageReport, UsageWindow, UsageWindowKind};
use crate::directory::home_dir;

const DEFAULT_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";
/// Claude Code's own `User-Agent` shape (the endpoint is meant for it).
const USER_AGENT: &str = "claude-code/2.1.0";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest answer read (the real one is a few KB).
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// Caps a corrupt `Retry-After` so it cannot silence usage for good.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// 1e10 separates epoch seconds (until 2286) from milliseconds (since 2001).
const EPOCH_MS_THRESHOLD: f64 = 10_000_000_000.0;

pub(super) const LABEL_FIVE_HOUR: &str = "5-hour limit";
pub(super) const LABEL_WEEK: &str = "Weekly · all models";

/// Why the endpoint gave no usage. Only [`OAuthError::RateLimited`] stops the
/// caller from falling back to the agent probe.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OAuthError {
    /// No readable login: the file is missing, unreadable or has no token.
    #[error("no Claude login: {0}")]
    NoCredentials(String),
    /// 401 / 403: the stored token is stale (it is not refreshed here).
    #[error("the usage API rejected the token (HTTP {0})")]
    Rejected(u16),
    /// 429: the endpoint asks to wait (`Retry-After`, if sent).
    #[error("the usage API is rate limited")]
    RateLimited { retry_after: Option<Duration> },
    /// Anything else: network, timeout, 5xx, a body that is not usage.
    #[error("the usage API gave no usage: {0}")]
    Unavailable(String),
}

/// The OAuth usage endpoint with the login of this machine.
#[derive(Debug, Clone)]
pub struct OAuthUsage {
    url: String,
    /// `None`: `CLAUDE_CONFIG_DIR`, else `~/.claude`, looked up at each request.
    config_dir: Option<PathBuf>,
    timeout: Duration,
}

impl Default for OAuthUsage {
    fn default() -> Self {
        Self::new()
    }
}

impl OAuthUsage {
    #[must_use]
    pub fn new() -> Self {
        Self {
            url: DEFAULT_URL.into(),
            config_dir: None,
            timeout: REQUEST_TIMEOUT,
        }
    }

    /// Asks `url` instead of Anthropic (tests).
    #[must_use]
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Reads `<dir>/.credentials.json` whatever `CLAUDE_CONFIG_DIR` says (tests).
    #[must_use]
    pub fn with_config_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(dir.into());
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// One request; `now_ms` stamps the report.
    pub async fn fetch(&self, now_ms: u64) -> Result<UsageReport, OAuthError> {
        let path = self.credentials_path()?;
        let credentials = read_credentials(&path)?;
        let body = self.request(&credentials.token).await?;
        report_from_json(&body, credentials.plan, now_ms)
    }

    fn credentials_path(&self) -> Result<PathBuf, OAuthError> {
        let dir = match &self.config_dir {
            Some(dir) => Some(dir.clone()),
            None => config_dir(
                std::env::var_os("CLAUDE_CONFIG_DIR")
                    .map(PathBuf::from)
                    .as_deref(),
                home_dir().as_deref(),
            ),
        };
        dir.map(|d| d.join(".credentials.json"))
            .ok_or_else(|| OAuthError::NoCredentials("no home directory".into()))
    }

    async fn request(&self, token: &str) -> Result<Value, OAuthError> {
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .user_agent(USER_AGENT)
            // The token goes to the endpoint named, nowhere else.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| unavailable(&format!("no HTTP client: {e}")))?;
        let response = client
            .get(&self.url)
            .bearer_auth(token)
            .header("anthropic-beta", BETA_HEADER)
            .send()
            .await
            .map_err(|e| unavailable(&describe_request_error(&e)))?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| parse_retry_after(v, crate::clock::now_ms()));
            return Err(classify_status(status.as_u16(), retry_after));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY_BYTES as u64)
        {
            return Err(unavailable("the answer is too large"));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| unavailable(&describe_request_error(&e)))?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(unavailable("the answer is too large"));
        }
        serde_json::from_slice(&bytes).map_err(|e| unavailable(&format!("not JSON: {e}")))
    }
}

fn unavailable(what: &str) -> OAuthError {
    OAuthError::Unavailable(what.into())
}

/// The cause of a failed request without its URL.
fn describe_request_error(e: &reqwest::Error) -> String {
    let kind = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else if e.is_body() || e.is_decode() {
        "could not read the answer"
    } else {
        "request failed"
    };
    kind.into()
}

/// The status of a non-2xx answer (Orca's `classifyClaudeOAuthUsageError`).
fn classify_status(status: u16, retry_after: Option<Duration>) -> OAuthError {
    match status {
        429 => OAuthError::RateLimited { retry_after },
        401 | 403 => OAuthError::Rejected(status),
        _ => OAuthError::Unavailable(format!("HTTP {status}")),
    }
}

/// `CLAUDE_CONFIG_DIR` if set (and not empty), else `<home>/.claude`.
fn config_dir(env: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    match env {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir.to_path_buf()),
        _ => home.map(|h| h.join(".claude")),
    }
}

struct Credentials {
    token: String,
    plan: Option<String>,
}

fn read_credentials(path: &Path) -> Result<Credentials, OAuthError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        OAuthError::NoCredentials(format!("cannot read {}: {}", path.display(), e.kind()))
    })?;
    parse_credentials(&text)
}

/// `claudeAiOauth.accessToken` and the plan (`subscriptionType`).
fn parse_credentials(text: &str) -> Result<Credentials, OAuthError> {
    let json: Value = serde_json::from_str(text)
        .map_err(|_| OAuthError::NoCredentials("the credentials file is not JSON".into()))?;
    let oauth = &json["claudeAiOauth"];
    let token = oauth["accessToken"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| OAuthError::NoCredentials("no access token in the credentials".into()))?;
    let plan = oauth["subscriptionType"]
        .as_str()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_lowercase);
    Ok(Credentials {
        token: token.into(),
        plan,
    })
}

/// The windows of an endpoint answer: `five_hour`, `seven_day` and the
/// `weekly_scoped` entries of `limits` (labelled like the `/usage` Markdown).
/// A window that is `null` or has no readable percentage is left out.
fn report_from_json(
    json: &Value,
    plan: Option<String>,
    now_ms: u64,
) -> Result<UsageReport, OAuthError> {
    let mut windows = Vec::new();
    let fixed = [
        ("five_hour", UsageWindowKind::FiveHour, LABEL_FIVE_HOUR),
        ("seven_day", UsageWindowKind::Week, LABEL_WEEK),
    ];
    for (key, kind, label) in fixed {
        if let Some(w) = window(&json[key], "utilization", kind, label.into()) {
            windows.push(w);
        }
    }
    for limit in json["limits"].as_array().into_iter().flatten() {
        if limit["kind"].as_str() != Some("weekly_scoped") {
            continue;
        }
        let name = limit["scope"]["model"]["display_name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        if let Some(name) = name
            && let Some(w) = window(
                limit,
                "percent",
                UsageWindowKind::Other,
                format!("Weekly · {name}"),
            )
        {
            windows.push(w);
        }
    }
    if windows.is_empty() {
        return Err(unavailable("the answer has no usage window"));
    }
    Ok(UsageReport {
        plan,
        windows,
        fetched_at_ms: now_ms,
    })
}

fn window(
    raw: &Value,
    percent_key: &str,
    kind: UsageWindowKind,
    label: String,
) -> Option<UsageWindow> {
    let percent = raw[percent_key].as_f64().filter(|p| p.is_finite())?;
    Some(UsageWindow {
        kind,
        label,
        percent: percent.clamp(0.0, 100.0),
        resets_at_ms: parse_reset(&raw["resets_at"]),
    })
}

/// An RFC 3339 string, or epoch seconds / milliseconds (a number or a numeric
/// string), as ms since the epoch.
fn parse_reset(value: &Value) -> Option<u64> {
    let ms = match value {
        Value::Number(n) => epoch_to_ms(n.as_f64()?),
        Value::String(s) => match s.trim().parse::<f64>() {
            Ok(n) => epoch_to_ms(n),
            Err(_) => parse_rfc3339_ms(s.trim()),
        },
        _ => None,
    }?;
    u64::try_from(ms).ok()
}

fn epoch_to_ms(n: f64) -> Option<i64> {
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let ms = if n > EPOCH_MS_THRESHOLD {
        n
    } else {
        n * 1000.0
    };
    // Within i64: a finite epoch value in ms is far below 2^63.
    #[allow(clippy::cast_possible_truncation)]
    Some(ms as i64)
}

/// `2026-11-26T17:00:00.360155+00:00` → ms since the epoch (the fraction is
/// truncated to ms; `Z` and `±HH:MM` offsets are read).
fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok().filter(|m| (1..=12).contains(m))?;
    let day: u32 = d.next()?.parse().ok().filter(|d| (1..=31).contains(d))?;
    if d.next().is_some() {
        return None;
    }
    let zone_at = rest.find(['Z', 'z', '+', '-'])?;
    let (clock, zone) = rest.split_at(zone_at);
    let (hms, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut t = hms.split(':');
    let hour: i64 = t.next()?.parse().ok().filter(|h| (0..24).contains(h))?;
    let minute: i64 = t.next()?.parse().ok().filter(|m| (0..60).contains(m))?;
    let second: i64 = t.next()?.parse().ok().filter(|s| (0..=60).contains(s))?;
    if t.next().is_some() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis: i64 = format!("{fraction:0<3}")[..3].parse().ok()?;
    let offset_min = match zone {
        "Z" | "z" => 0,
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (h, m) = zone[1..].split_once(':')?;
            let h: i64 = h.parse().ok().filter(|h| (0..24).contains(h))?;
            let m: i64 = m.parse().ok().filter(|m| (0..60).contains(m))?;
            sign * (h * 60 + m)
        }
    };
    let days = days_from_civil(year, month, day);
    Some(
        days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + millis
            - offset_min * 60_000,
    )
}

/// `Retry-After` (RFC 9110): delta-seconds or an HTTP date. `None` when the
/// value is not a wait.
fn parse_retry_after(value: &str, now_ms: u64) -> Option<Duration> {
    let value = value.trim();
    let wait = match value.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() && seconds > 0.0 => {
            Duration::from_secs_f64(seconds.min(MAX_RETRY_AFTER.as_secs_f64()))
        }
        Ok(_) => return None,
        Err(_) => {
            let at = parse_http_date_ms(value)?;
            let wait = u64::try_from(at).ok()?.checked_sub(now_ms)?;
            Duration::from_millis(wait)
        }
    };
    (!wait.is_zero()).then_some(wait.min(MAX_RETRY_AFTER))
}

/// `Sun, 06 Nov 1994 08:49:37 GMT` (the IMF-fixdate form) → ms since the epoch.
fn parse_http_date_ms(s: &str) -> Option<i64> {
    let (_, rest) = s.split_once(", ")?;
    let mut parts = rest.split(' ');
    let day: u32 = parts
        .next()?
        .parse()
        .ok()
        .filter(|d| (1..=31).contains(d))?;
    let month = super::parse::month_number(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    let clock = parts.next()?;
    if parts.next()? != "GMT" || parts.next().is_some() {
        return None;
    }
    let mut t = clock.split(':');
    let hour: i64 = t.next()?.parse().ok().filter(|h| (0..24).contains(h))?;
    let minute: i64 = t.next()?.parse().ok().filter(|m| (0..60).contains(m))?;
    let second: i64 = t.next()?.parse().ok().filter(|s| (0..=60).contains(s))?;
    Some(
        days_from_civil(year, month, day) * 86_400_000
            + hour * 3_600_000
            + minute * 60_000
            + second * 1000,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// 2026-11-23T00:00:00Z
    const NOW: u64 = 1_795_392_000_000;
    /// 2026-11-26T17:00:00Z
    const NOV_26_17H: u64 = 1_795_712_400_000;
    const FIXTURE: &str = include_str!("testdata/oauth-usage.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture")
    }

    // ---- JSON → windows -------------------------------------------------

    #[test]
    fn a_null_five_hour_window_is_left_out_and_the_scoped_weekly_is_labelled_by_model() {
        let r = report_from_json(&fixture(), Some("max".into()), NOW).expect("report");
        assert_eq!(r.plan.as_deref(), Some("max"));
        assert_eq!(r.fetched_at_ms, NOW);
        let labels: Vec<_> = r.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["Weekly · all models", "Weekly · Fable"]);
        let week = r.window(UsageWindowKind::Week).expect("week");
        assert!((week.percent - 43.0).abs() < f64::EPSILON);
        // 2026-11-26T17:00:00.360155+00:00 (the fraction is cut to ms)
        assert_eq!(week.resets_at_ms, Some(NOV_26_17H + 360));
        assert_eq!(r.windows[1].kind, UsageWindowKind::Other);
        assert!(r.windows[1].percent.abs() < f64::EPSILON);
        assert!(r.window(UsageWindowKind::FiveHour).is_none());
    }

    #[test]
    fn the_labels_match_what_the_markdown_path_reads() {
        // `Weekly · Fable` and the plan of the Markdown fixture, window by window.
        let md = parse_md_fixture();
        let oauth = report_from_json(&fixture(), Some("max".into()), NOW).expect("report");
        assert_eq!(md.plan, oauth.plan);
        let md_windows: Vec<_> = md.windows.iter().map(|w| (&w.label, w.kind)).collect();
        let oauth_windows: Vec<_> = oauth.windows.iter().map(|w| (&w.label, w.kind)).collect();
        assert_eq!(md_windows, oauth_windows);
    }

    fn parse_md_fixture() -> UsageReport {
        super::super::parse_usage_markdown(include_str!("testdata/usage-structured.md"), NOW)
            .expect("markdown")
    }

    #[test]
    fn five_hour_and_percentages_are_clamped_to_0_100() {
        let json = json!({
            "five_hour": {"utilization": 120.5, "resets_at": "2026-11-23T05:00:00Z"},
            "seven_day": {"utilization": -3.0, "resets_at": null},
        });
        let r = report_from_json(&json, None, NOW).expect("report");
        assert_eq!(r.plan, None);
        let five = r.window(UsageWindowKind::FiveHour).expect("5h");
        assert!((five.percent - 100.0).abs() < f64::EPSILON);
        assert_eq!(five.resets_at_ms, Some(NOW + 5 * 3_600_000));
        let week = r.window(UsageWindowKind::Week).expect("week");
        assert!(week.percent.abs() < f64::EPSILON);
        assert_eq!(week.resets_at_ms, None);
    }

    #[test]
    fn windows_without_a_percentage_or_a_model_name_are_left_out() {
        let json = json!({
            "five_hour": {"resets_at": "2026-11-23T05:00:00Z"},
            "seven_day": {"utilization": 10.0},
            "limits": [
                {"kind": "weekly_all", "percent": 10, "scope": null},
                {"kind": "weekly_scoped", "percent": 5, "scope": null},
                {"kind": "weekly_scoped", "percent": 6, "scope": {"model": {"display_name": " "}}},
                {"kind": "weekly_scoped", "scope": {"model": {"display_name": "Opus"}}},
                {"kind": "weekly_scoped", "percent": 7, "scope": {"model": {"display_name": "Opus"}}},
            ],
        });
        let r = report_from_json(&json, None, NOW).expect("report");
        let labels: Vec<_> = r.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["Weekly · all models", "Weekly · Opus"]);
    }

    #[test]
    fn an_answer_without_any_window_is_not_a_report() {
        let err = report_from_json(&json!({"five_hour": null, "seven_day": null}), None, NOW)
            .expect_err("nothing");
        assert!(matches!(err, OAuthError::Unavailable(_)), "{err:?}");
        assert!(report_from_json(&json!([]), None, NOW).is_err());
    }

    #[test]
    fn reset_times_are_read_in_every_form() {
        let want = Some(NOV_26_17H);
        let secs = NOV_26_17H / 1000;
        for value in [
            json!("2026-11-26T17:00:00Z"),
            json!("2026-11-26T17:00:00.000+00:00"),
            json!("2026-11-27T02:00:00+09:00"),
            json!("2026-11-26T12:00:00-05:00"),
            json!(secs),
            json!(NOV_26_17H),
            json!(secs.to_string()),
            json!(NOV_26_17H.to_string()),
        ] {
            assert_eq!(parse_reset(&value), want, "{value}");
        }
        assert_eq!(
            parse_reset(&json!("2026-11-26T17:00:00.5Z")),
            Some(NOV_26_17H + 500)
        );
        for bad in [
            json!(null),
            json!(""),
            json!("soon"),
            json!("2026-13-26T17:00:00Z"),
            json!("2026-11-26T25:00:00Z"),
            json!("2026-11-26T17:00:00"),
            json!(-5),
            json!(true),
        ] {
            assert_eq!(parse_reset(&bad), None, "{bad}");
        }
    }

    #[test]
    fn retry_after_is_seconds_or_an_http_date_and_capped() {
        assert_eq!(parse_retry_after("30", NOW), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after("999999999", NOW), Some(MAX_RETRY_AFTER));
        // Mon, 23 Nov 2026 00:01:00 GMT is a minute after NOW.
        assert_eq!(
            parse_retry_after("Mon, 23 Nov 2026 00:01:00 GMT", NOW),
            Some(Duration::from_secs(60))
        );
        for none in [
            "0",
            "-1",
            "",
            "soon",
            "Mon, 23 Nov 2026 00:00:00 GMT",
            "Sun, 22 Nov 2026 00:00:00 GMT",
        ] {
            assert_eq!(parse_retry_after(none, NOW), None, "{none:?}");
        }
    }

    // ---- credentials -----------------------------------------------------

    #[test]
    fn the_config_dir_is_claude_config_dir_else_home_dot_claude() {
        let home = Path::new("/home/u");
        assert_eq!(
            config_dir(Some(Path::new("/cfg")), Some(home)),
            Some(PathBuf::from("/cfg"))
        );
        for env in [None, Some(Path::new(""))] {
            assert_eq!(
                config_dir(env, Some(home)),
                Some(PathBuf::from("/home/u/.claude"))
            );
        }
        assert_eq!(config_dir(None, None), None);
    }

    #[test]
    fn the_token_and_the_plan_are_read_from_the_credentials_file() {
        let text = r#"{"claudeAiOauth": {"accessToken": " tok-1 ", "refreshToken": "r",
            "subscriptionType": "Max", "rateLimitTier": "default_claude_max_20x"}}"#;
        let c = parse_credentials(text).expect("credentials");
        assert_eq!(c.token, "tok-1");
        assert_eq!(c.plan.as_deref(), Some("max"));
        let c = parse_credentials(r#"{"claudeAiOauth": {"accessToken": "t"}}"#).expect("no plan");
        assert_eq!(c.plan, None);
    }

    #[test]
    fn missing_or_broken_credentials_are_no_credentials() {
        for text in [
            "",
            "not json",
            "{}",
            r#"{"claudeAiOauth": {}}"#,
            r#"{"claudeAiOauth": {"accessToken": ""}}"#,
            r#"{"claudeAiOauth": {"accessToken": 5}}"#,
        ] {
            let err = parse_credentials(text).err().expect(text);
            assert!(
                matches!(err, OAuthError::NoCredentials(_)),
                "{text}: {err:?}"
            );
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let err = read_credentials(&dir.path().join(".credentials.json"))
            .err()
            .expect("no file");
        assert!(matches!(err, OAuthError::NoCredentials(_)), "{err:?}");
    }

    #[tokio::test]
    async fn fetch_without_a_credentials_file_does_not_call_the_endpoint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (url, server) = serve(Reply::Hang).await;
        let err = OAuthUsage::new()
            .with_url(url)
            .with_config_dir(dir.path())
            .fetch(NOW)
            .await
            .expect_err("no login");
        assert!(matches!(err, OAuthError::NoCredentials(_)), "{err:?}");
        assert!(!server.was_called(), "no request without a token");
    }

    // ---- HTTP -------------------------------------------------------------

    enum Reply {
        Status(u16, Vec<(&'static str, String)>, String),
        /// Accepts the connection and never answers.
        Hang,
    }

    struct Server {
        calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Server {
        fn was_called(&self) -> bool {
            !self.calls.lock().expect("calls").is_empty()
        }

        fn request(&self) -> String {
            self.calls
                .lock()
                .expect("calls")
                .first()
                .cloned()
                .expect("a request")
        }
    }

    /// A one-connection-at-a-time HTTP server on localhost that answers every
    /// request with `reply`; its URL and a record of the requests.
    async fn serve(reply: Reply) -> (String, Server) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!(
            "http://{}/api/oauth/usage",
            listener.local_addr().expect("addr")
        );
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = calls.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                recorded
                    .lock()
                    .expect("calls")
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                match &reply {
                    Reply::Hang => held.push(socket),
                    Reply::Status(code, headers, body) => {
                        let mut head = format!(
                            "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                            body.len()
                        );
                        for (k, v) in headers {
                            head.push_str(&format!("{k}: {v}\r\n"));
                        }
                        head.push_str("\r\n");
                        let _ = socket.write_all(head.as_bytes()).await;
                        let _ = socket.write_all(body.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    }
                }
            }
        });
        (url, Server { calls })
    }

    fn login(dir: &Path) {
        std::fs::write(
            dir.join(".credentials.json"),
            r#"{"claudeAiOauth": {"accessToken": "secret-token", "subscriptionType": "max"}}"#,
        )
        .expect("write credentials");
    }

    async fn fetch_from(
        reply: Reply,
        timeout: Duration,
    ) -> (Result<UsageReport, OAuthError>, Server) {
        let dir = tempfile::tempdir().expect("tempdir");
        login(dir.path());
        let (url, server) = serve(reply).await;
        let result = OAuthUsage::new()
            .with_url(url)
            .with_config_dir(dir.path())
            .with_timeout(timeout)
            .fetch(NOW)
            .await;
        (result, server)
    }

    fn status(code: u16, headers: Vec<(&'static str, String)>, body: &str) -> Reply {
        Reply::Status(code, headers, body.into())
    }

    #[tokio::test]
    async fn a_200_gives_the_report_and_the_request_carries_the_token_and_beta_header() {
        let (result, server) =
            fetch_from(status(200, vec![], FIXTURE), Duration::from_secs(5)).await;
        let r = result.expect("report");
        assert_eq!(r.plan.as_deref(), Some("max"));
        assert_eq!(r.windows.len(), 2);
        let request = server.request().to_lowercase();
        assert!(request.starts_with("get /api/oauth/usage "), "{request}");
        assert!(
            request.contains("authorization: bearer secret-token"),
            "{request}"
        );
        assert!(
            request.contains("anthropic-beta: oauth-2025-04-20"),
            "{request}"
        );
        assert!(request.contains("user-agent: claude-code/"), "{request}");
    }

    #[tokio::test]
    async fn a_401_and_a_403_are_a_stale_token() {
        for code in [401, 403] {
            let (result, _) = fetch_from(status(code, vec![], "{}"), Duration::from_secs(5)).await;
            assert_eq!(result.err(), Some(OAuthError::Rejected(code)));
        }
    }

    #[tokio::test]
    async fn a_429_carries_retry_after() {
        let reply = status(429, vec![("retry-after", "120".into())], "{}");
        let (result, _) = fetch_from(reply, Duration::from_secs(5)).await;
        assert_eq!(
            result.err(),
            Some(OAuthError::RateLimited {
                retry_after: Some(Duration::from_secs(120))
            })
        );
        let (result, _) = fetch_from(status(429, vec![], "{}"), Duration::from_secs(5)).await;
        assert_eq!(
            result.err(),
            Some(OAuthError::RateLimited { retry_after: None })
        );
    }

    #[tokio::test]
    async fn server_errors_bad_json_and_timeouts_are_unavailable() {
        for reply in [
            status(500, vec![], "{}"),
            status(503, vec![], "oops"),
            status(404, vec![], "{}"),
            status(200, vec![], "not json"),
            status(200, vec![], r#"{"five_hour": null}"#),
        ] {
            let (result, _) = fetch_from(reply, Duration::from_secs(5)).await;
            assert!(
                matches!(result, Err(OAuthError::Unavailable(_))),
                "{result:?}"
            );
        }
        let (result, _) = fetch_from(Reply::Hang, Duration::from_millis(200)).await;
        let Err(OAuthError::Unavailable(why)) = result else {
            panic!("timeout expected, got {result:?}");
        };
        assert!(why.contains("timed out"), "{why}");
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_is_unavailable() {
        let dir = tempfile::tempdir().expect("tempdir");
        login(dir.path());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}/", listener.local_addr().expect("addr"));
        drop(listener);
        let err = OAuthUsage::new()
            .with_url(url)
            .with_config_dir(dir.path())
            .fetch(NOW)
            .await
            .expect_err("refused");
        assert!(matches!(err, OAuthError::Unavailable(_)), "{err:?}");
        assert!(!err.to_string().contains("secret-token"));
    }

    /// Reads the real usage with the login of this machine (`--ignored`; the
    /// token is never printed).
    #[tokio::test]
    #[ignore = "uses the real Claude login and the network"]
    async fn the_real_login_reads_the_usage() {
        let report = OAuthUsage::new()
            .fetch(crate::clock::now_ms())
            .await
            .expect("usage through the real login");
        eprintln!("usage: {report:?}");
        assert!(!report.windows.is_empty());
    }
}
