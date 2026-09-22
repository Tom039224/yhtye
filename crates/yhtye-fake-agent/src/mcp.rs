//! MCP client side of the fake agent: calls the HTTP MCP server given in
//! `session/new` (the `mcp_call` / `mcp_list` actions).
//!
//! String arguments may contain `${name}` placeholders. Variables come from
//! `key=value` tokens in the current prompt (e.g. `help_id=H-1` in an inbox
//! message) and from top-level fields of earlier successful tool results
//! (e.g. `group_id` returned by `create_group`).

use std::collections::HashMap;

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::Value;

pub type Vars = HashMap<String, String>;

/// Records `key=value` tokens of `prompt` (value up to the next whitespace).
pub fn capture_prompt_vars(prompt: &str, vars: &mut Vars) {
    for token in prompt.split_whitespace() {
        if let Some((k, v)) = token.split_once('=')
            && !k.is_empty()
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            vars.insert(k.to_string(), v.to_string());
        }
    }
}

/// Records top-level string / number fields of a tool result.
pub fn capture_result_vars(result: &Value, vars: &mut Vars) {
    let Some(obj) = result.as_object() else {
        return;
    };
    for (k, v) in obj {
        match v {
            Value::String(s) => {
                vars.insert(k.clone(), s.clone());
            }
            Value::Number(n) => {
                vars.insert(k.clone(), n.to_string());
            }
            _ => {}
        }
    }
}

/// Replaces `${name}` in every string of `value`.
pub fn substitute(value: &Value, vars: &Vars) -> Value {
    match value {
        Value::String(s) => Value::String(substitute_str(s, vars)),
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, vars)).collect()),
        Value::Object(obj) => Value::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), substitute(v, vars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn substitute_str(s: &str, vars: &Vars) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    out
}

async fn connect(url: &str) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, String> {
    let transport = StreamableHttpClientTransport::from_uri(url.to_string());
    ().serve(transport)
        .await
        .map_err(|e| format!("connect: {e}"))
}

/// Calls `tool` once over a fresh client connection.
pub async fn call_tool(url: &str, tool: &str, args: &Value) -> Result<CallToolResult, String> {
    let client = connect(url).await?;
    let params = CallToolRequestParams::new(tool.to_string())
        .with_arguments(args.as_object().cloned().unwrap_or_default());
    let result = client.call_tool(params).await.map_err(|e| e.to_string());
    let _ = client.cancel().await;
    result
}

/// Names of the tools the server lists for this session.
pub async fn list_tools(url: &str) -> Result<Vec<String>, String> {
    let client = connect(url).await?;
    let result = client
        .list_all_tools()
        .await
        .map(|tools| tools.into_iter().map(|t| t.name.to_string()).collect())
        .map_err(|e| e.to_string());
    let _ = client.cancel().await;
    result
}

/// `mcp:<tool>:ok:<json>` / `mcp:<tool>:error:<json>` report of a call.
pub fn report(tool: &str, result: &Result<CallToolResult, String>) -> String {
    match result {
        Ok(r) => {
            let body = r.structured_content.clone().unwrap_or(Value::Null);
            let status = if r.is_error == Some(true) {
                "error"
            } else {
                "ok"
            };
            format!("mcp:{tool}:{status}:{body}")
        }
        Err(e) => format!("mcp:{tool}:failed:{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_prompt_and_result_vars() {
        let mut vars = Vars::new();
        capture_prompt_vars(
            "[yhtye:help_raised] help_id=H-1 task=T-1 kind=blocked\nbody",
            &mut vars,
        );
        capture_result_vars(
            &json!({"group_id": "G-1", "n": 2, "x": {"y": 1}}),
            &mut vars,
        );
        let args = json!({"help_id": "${help_id}", "ids": ["${group_id}/${n}"], "keep": "${nope}"});
        assert_eq!(
            substitute(&args, &vars),
            json!({"help_id": "H-1", "ids": ["G-1/2"], "keep": "${nope}"})
        );
    }
}
