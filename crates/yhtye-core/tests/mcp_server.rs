//! MCP server over real HTTP with an rmcp client: token routing, role-based
//! tool lists, forbidden / invalid-argument errors, and calls reaching the port.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::{Value, json};
use yhtye_core::domain::{Role, ToolError};
use yhtye_core::mcp::{McpHost, SessionBinding, TokenRegistry, ToolCall, ToolPort};

/// Records calls and echoes them back.
#[derive(Default)]
struct EchoPort {
    calls: Mutex<Vec<(SessionBinding, ToolCall)>>,
}

#[async_trait]
impl ToolPort for EchoPort {
    async fn call(&self, binding: SessionBinding, call: ToolCall) -> Result<Value, ToolError> {
        let echoed = serde_json::to_value(&call).map_err(|e| ToolError::internal(e.to_string()))?;
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((binding, call));
        Ok(json!({ "echo": echoed }))
    }
}

struct Fixture {
    host: McpHost,
    port: Arc<EchoPort>,
}

async fn fixture() -> Fixture {
    let port = Arc::new(EchoPort::default());
    let host = McpHost::start(
        "127.0.0.1:0".parse().expect("addr"),
        TokenRegistry::new(),
        port.clone(),
        None,
    )
    .await
    .expect("server starts");
    Fixture { host, port }
}

fn sub_binding(role: Role) -> SessionBinding {
    SessionBinding {
        session: "T-1/0".into(),
        role,
        project: "P-1".into(),
        chat: None,
        group: Some("G-1".into()),
        task: Some("T-1".into()),
        step: Some(0),
    }
}

async fn connect(url: String) -> RunningService<rmcp::RoleClient, ()> {
    let transport = StreamableHttpClientTransport::from_uri(url);
    ().serve(transport).await.expect("client connects")
}

async fn call(
    client: &RunningService<rmcp::RoleClient, ()>,
    tool: &str,
    args: Value,
) -> CallToolResult {
    let params = CallToolRequestParams::new(tool.to_string())
        .with_arguments(args.as_object().cloned().unwrap_or_default());
    client.call_tool(params).await.expect("tools/call answered")
}

fn error_code(res: &CallToolResult) -> Option<String> {
    assert_eq!(res.is_error, Some(true), "{res:?}");
    res.structured_content
        .as_ref()?
        .pointer("/error/code")?
        .as_str()
        .map(str::to_string)
}

#[tokio::test]
async fn unknown_token_is_404() {
    let f = fixture().await;
    let url = format!("http://{}/mcp/{}", f.host.addr(), "0".repeat(32));
    let res = reqwest::Client::new()
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#)
        .send()
        .await
        .expect("request");
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);
    f.host.shutdown().await;
}

#[tokio::test]
async fn tool_list_depends_on_role() {
    let f = fixture().await;
    let orch = f
        .host
        .registry()
        .issue(SessionBinding::orchestrator("orch", "P-1", "C-1"));
    let sub = f.host.registry().issue(sub_binding(Role::Implementer));

    let client = connect(f.host.url(&orch)).await;
    let names: Vec<_> = client
        .list_all_tools()
        .await
        .expect("list")
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains(&"create_task".to_string()), "{names:?}");
    assert!(!names.contains(&"report_step_done".to_string()));
    client.cancel().await.expect("close");

    let client = connect(f.host.url(&sub)).await;
    let names: Vec<_> = client
        .list_all_tools()
        .await
        .expect("list")
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(names, vec!["report_step_done", "help"]);
    client.cancel().await.expect("close");
    f.host.shutdown().await;
}

#[tokio::test]
async fn calls_reach_the_port_with_the_binding() {
    let f = fixture().await;
    let binding = sub_binding(Role::Reviewer);
    let token = f.host.registry().issue(binding.clone());
    let client = connect(f.host.url(&token)).await;

    let res = call(
        &client,
        "report_step_done",
        json!({"result": "looks good", "verdict": "approve"}),
    )
    .await;
    assert_eq!(res.is_error, Some(false));
    let structured = res.structured_content.expect("structured");
    assert_eq!(
        structured.pointer("/echo/tool"),
        Some(&json!("report_step_done"))
    );
    let text = res.content[0].as_text().expect("text").text.clone();
    assert_eq!(
        serde_json::from_str::<Value>(&text).expect("json text"),
        structured
    );
    let calls = f.port.calls.lock().expect("lock").clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, binding);
    client.cancel().await.expect("close");
    f.host.shutdown().await;
}

#[tokio::test]
async fn wrong_role_is_forbidden_and_bad_args_are_invalid_argument() {
    let f = fixture().await;
    let orch = f
        .host
        .registry()
        .issue(SessionBinding::orchestrator("orch", "P-1", "C-1"));
    let client = connect(f.host.url(&orch)).await;

    let res = call(&client, "report_step_done", json!({"result": "x"})).await;
    assert_eq!(error_code(&res).as_deref(), Some("forbidden"));

    let res = call(&client, "create_task", json!({"group_id": "G-1"})).await;
    assert_eq!(error_code(&res).as_deref(), Some("invalid_argument"));

    let res = call(&client, "create_group", json!({"title": 3})).await;
    assert_eq!(error_code(&res).as_deref(), Some("invalid_argument"));

    let unknown = client
        .call_tool(CallToolRequestParams::new("no_such_tool"))
        .await;
    assert!(unknown.is_err(), "unknown tool is a protocol error");

    assert!(f.port.calls.lock().expect("lock").is_empty());
    client.cancel().await.expect("close");
    f.host.shutdown().await;
}

#[tokio::test]
async fn revoked_token_is_404() {
    let f = fixture().await;
    let token = f.host.registry().issue(sub_binding(Role::Implementer));
    let client = connect(f.host.url(&token)).await;
    f.host.registry().revoke(&token);
    let params = CallToolRequestParams::new("help").with_arguments(
        json!({"kind":"blocked","message":"x"})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    );
    assert!(client.call_tool(params).await.is_err());
    let _ = client.cancel().await;
    f.host.shutdown().await;
}

/// Claude Code 2.1.280 speaks the 2026-07-28 protocol, which requires cache hints
/// on list results; without them it rejects `tools/list` as INVALID_RESULT.
#[tokio::test]
async fn modern_protocol_tool_list_has_cache_hints() {
    let f = fixture().await;
    let token = f.host.registry().issue(sub_binding(Role::Implementer));
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/list",
        "params": {"_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "t", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities": {}
        }}
    });
    let res = reqwest::Client::new()
        .post(f.host.url(&token))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/list")
        .body(body.to_string())
        .send()
        .await
        .expect("request");
    let status = res.status();
    let text = res.text().await.expect("body");
    assert!(status.is_success(), "{status}: {text}");
    let json: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    let result = &json["result"];
    assert_eq!(result["ttlMs"], json!(0), "{json}");
    assert_eq!(result["cacheScope"], json!("private"), "{json}");
    assert_eq!(result["tools"].as_array().map(Vec::len), Some(2), "{json}");
    f.host.shutdown().await;
}

/// Requests from web pages (they carry `Origin`) are refused even with a valid
/// token and an allowed Host; agents send no `Origin` (Stage 6b review).
#[tokio::test]
async fn browser_requests_with_an_origin_are_forbidden() {
    let f = fixture().await;
    let token = f.host.registry().issue(sub_binding(Role::Implementer));
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    let post = |origin: Option<&'static str>| {
        let mut req = reqwest::Client::new()
            .post(f.host.url(&token))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(body.to_string());
        if let Some(o) = origin {
            req = req.header("origin", o);
        }
        req.send()
    };
    let res = post(Some("http://evil.example")).await.expect("request");
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
    let res = post(Some("http://localhost:1420")).await.expect("request");
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
    let res = post(None).await.expect("request");
    assert_ne!(res.status(), reqwest::StatusCode::FORBIDDEN);
    f.host.shutdown().await;
}
