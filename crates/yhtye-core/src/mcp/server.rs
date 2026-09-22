//! The in-process MCP server: rmcp streamable HTTP mounted on axum at
//! `/mcp/{token}` (`core-design.md` §4, `mcp-tools.md` §1).

use std::net::SocketAddr;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{McpServer, McpServerHttp};
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use super::binding::{McpToken, SessionBinding, TokenRegistry};
use super::port::ToolPort;
use super::tools::{ToolName, parse_call, tools_for};
use crate::domain::ToolError;

/// MCP server name given to agents; Claude Code exposes tools as `mcp__yhtye__<tool>`.
pub const MCP_SERVER_NAME: &str = "yhtye";

/// One tool call as seen by the server (for logging / observation).
#[derive(Debug, Clone, Serialize, TS)]
pub struct ToolCallRecord {
    pub binding: SessionBinding,
    pub tool: String,
    pub args: serde_json::Value,
    pub result: Result<serde_json::Value, ToolError>,
}

/// Token of the current HTTP request, put into request extensions by the middleware.
#[derive(Debug, Clone)]
struct RequestToken(String);

#[derive(Clone)]
struct Handler {
    registry: TokenRegistry,
    port: Arc<dyn ToolPort>,
    observer: Option<mpsc::UnboundedSender<ToolCallRecord>>,
}

impl Handler {
    fn binding(&self, ctx: &RequestContext<RoleServer>) -> Result<SessionBinding, ErrorData> {
        ctx.extensions
            .get::<http::request::Parts>()
            .and_then(|p| p.extensions.get::<RequestToken>())
            .and_then(|t| self.registry.get(&t.0))
            .ok_or_else(|| ErrorData::invalid_request("unknown or revoked session token", None))
    }

    async fn run_tool(
        &self,
        binding: &SessionBinding,
        tool: ToolName,
        args: Option<rmcp::model::JsonObject>,
    ) -> Result<serde_json::Value, ToolError> {
        if !tool.allowed_for(binding.role) {
            return Err(ToolError::forbidden(format!(
                "{} is not available to the {} role",
                tool.as_str(),
                binding.role
            )));
        }
        let call = parse_call(tool, args)?;
        self.port.call(binding.clone(), call).await
    }

    fn observe(&self, record: ToolCallRecord) {
        match &record.result {
            Ok(_) => {
                tracing::info!(session = %record.binding.session, tool = %record.tool, "tool call ok")
            }
            Err(e) => {
                tracing::info!(session = %record.binding.session, tool = %record.tool, error = %e, "tool call failed")
            }
        }
        if let Some(tx) = &self.observer {
            let _ = tx.send(record);
        }
    }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new(MCP_SERVER_NAME, env!("CARGO_PKG_VERSION")),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let binding = self.binding(&context)?;
        // The 2026-07-28 protocol (Claude Code 2.1.280 negotiates it) requires the
        // cache hints; clients reject the list as INVALID_RESULT without them.
        // The list depends on the caller's token, hence private and uncached.
        Ok(ListToolsResult::with_all_items(tools_for(binding.role))
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let binding = self.binding(&context)?;
        let Some(tool) = ToolName::from_name(&request.name) else {
            return Err(ErrorData::invalid_params(
                format!("unknown tool {}", request.name),
                None,
            ));
        };
        let args = request.arguments.clone().unwrap_or_default();
        let result = self.run_tool(&binding, tool, request.arguments).await;
        let response = match &result {
            Ok(value) => CallToolResult::structured(value.clone()),
            Err(e) => CallToolResult::structured_error(e.body()),
        };
        self.observe(ToolCallRecord {
            binding,
            tool: tool.as_str().into(),
            args: serde_json::Value::Object(args),
            result,
        });
        Ok(response.into())
    }
}

/// Rejects unknown tokens with 404 before rmcp sees the request.
async fn check_token(
    State(registry): State<TokenRegistry>,
    Path(token): Path<String>,
    mut req: Request,
    next: Next,
) -> Response {
    if registry.get(&token).is_none() {
        return (StatusCode::NOT_FOUND, "unknown session token").into_response();
    }
    req.extensions_mut().insert(RequestToken(token));
    next.run(req).await
}

/// The running MCP server. Dropping it does not stop the server; call [`McpHost::shutdown`].
pub struct McpHost {
    addr: SocketAddr,
    registry: TokenRegistry,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl McpHost {
    /// Binds `bind` (use `127.0.0.1:0`) and serves tools through `port`.
    /// Tool calls are also reported to `observer` if given.
    pub async fn start(
        bind: SocketAddr,
        registry: TokenRegistry,
        port: Arc<dyn ToolPort>,
        observer: Option<mpsc::UnboundedSender<ToolCallRecord>>,
    ) -> std::io::Result<Self> {
        let cancel = CancellationToken::new();
        let handler = Handler {
            registry: registry.clone(),
            port,
            observer,
        };
        // Stateless: every request is identified by the token in its URL, so no MCP
        // session state (and no idle keep-alive that could expire during a long task).
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_cancellation_token(cancel.child_token());
        let service: StreamableHttpService<Handler, LocalSessionManager> =
            StreamableHttpService::new(move || Ok(handler.clone()), Arc::default(), config);
        let app = axum::Router::new()
            .route_service("/mcp/{token}", service)
            .route_layer(middleware::from_fn_with_state(
                registry.clone(),
                check_token,
            ));
        let listener = tokio::net::TcpListener::bind(bind).await?;
        let addr = listener.local_addr()?;
        let shutdown = cancel.clone();
        let task = tokio::spawn(async move {
            let served = axum::serve(listener, app)
                .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                .await;
            if let Err(e) = served {
                tracing::error!("MCP server stopped: {e}");
            }
        });
        Ok(Self {
            addr,
            registry,
            cancel,
            task,
        })
    }

    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    #[must_use]
    pub fn registry(&self) -> &TokenRegistry {
        &self.registry
    }

    /// `http://127.0.0.1:<port>/mcp/<token>`.
    #[must_use]
    pub fn url(&self, token: &McpToken) -> String {
        format!("http://{}/mcp/{}", self.addr, token.as_str())
    }

    /// The server entry to pass in ACP `session/new` for a session holding `token`.
    #[must_use]
    pub fn acp_server(&self, token: &McpToken) -> McpServer {
        McpServer::Http(McpServerHttp::new(MCP_SERVER_NAME, self.url(token)))
    }

    /// Stops accepting requests and waits for the server task.
    pub async fn shutdown(self) {
        self.cancel.cancel();
        if let Err(e) = self.task.await {
            tracing::error!("MCP server task panicked: {e}");
        }
    }
}
