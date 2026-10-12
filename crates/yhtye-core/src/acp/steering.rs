//! The adapters' `_session/steering` extension: a message injected into the
//! turn that is running (`acp-harnesses.md` §4.5).
//!
//! An agent advertises it with `_meta.steering.supported: true` at the top
//! level of the `initialize` result; Yhtye never probes for it (an agent
//! without it answers "method not found").

use agent_client_protocol::schema::v1::{ContentBlock, Meta, SessionId};
use serde::{Deserialize, Serialize};

use super::events::AgentError;

pub(crate) const STEERING_METHOD: &str = "_session/steering";

/// What became of a steering message ([`super::AgentHandle::steer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SteerOutcome {
    /// Joined the running turn. Its result arrives with that turn's
    /// `TurnEnded`; there is no second turn.
    Injected,
    /// No turn was running, so nothing happened: send the content as a normal
    /// prompt instead.
    PromptRequired,
    /// The agent started a turn of its own for the message (an agent that
    /// ignores `idleBehavior`, e.g. Codex). Yhtye does not track that turn: no
    /// `TurnEnded` follows for it.
    StartedNewTurn,
}

/// `_session/steering` params. `idleBehavior: promptRequired` asks the agent
/// not to start a turn of its own when none is running.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcRequest)]
#[request(method = "_session/steering", response = SteeringResponse)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SteeringRequest {
    session_id: SessionId,
    prompt: Vec<ContentBlock>,
    #[serde(rename = "_meta")]
    meta: serde_json::Value,
}

impl SteeringRequest {
    pub fn new(session_id: SessionId, prompt: Vec<ContentBlock>) -> Self {
        Self {
            session_id,
            prompt,
            meta: serde_json::json!({ "steering": { "idleBehavior": "promptRequired" } }),
        }
    }
}

/// `_session/steering` result. Other fields (Claude's `reason`) are ignored.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcResponse)]
pub(crate) struct SteeringResponse {
    outcome: String,
}

impl SteeringResponse {
    /// The outcome; one Yhtye does not know (Codex's `failed`) is an error.
    pub fn into_outcome(self) -> Result<SteerOutcome, AgentError> {
        match self.outcome.as_str() {
            "injected" => Ok(SteerOutcome::Injected),
            "promptRequired" => Ok(SteerOutcome::PromptRequired),
            "startedNewTurn" => Ok(SteerOutcome::StartedNewTurn),
            other => Err(AgentError::Request {
                method: STEERING_METHOD.into(),
                message: format!("the agent did not apply the message (outcome `{other}`)"),
            }),
        }
    }
}

/// Whether the top-level `_meta` of the `initialize` result advertises steering.
pub(crate) fn advertised(meta: Option<&Meta>) -> bool {
    meta.and_then(|m| m.get("steering"))
        .and_then(|s| s.get("supported"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::JsonRpcMessage;
    use agent_client_protocol::schema::v1::TextContent;
    use serde_json::json;

    use super::*;

    #[test]
    fn steering_is_advertised_only_by_a_true_supported_flag() {
        let meta = |v: serde_json::Value| v.as_object().cloned();
        assert!(advertised(
            meta(json!({"steering": {"supported": true}})).as_ref()
        ));
        assert!(!advertised(None));
        assert!(!advertised(meta(json!({})).as_ref()));
        assert!(!advertised(
            meta(json!({"steering": {"supported": false}})).as_ref()
        ));
        assert!(!advertised(
            meta(json!({"steering": {"supported": "yes"}})).as_ref()
        ));
        assert!(!advertised(meta(json!({"steering": true})).as_ref()));
    }

    #[test]
    fn the_request_carries_the_prompt_and_asks_for_prompt_required_when_idle() {
        let req = SteeringRequest::new(
            "s-1".to_string().into(),
            vec![ContentBlock::Text(TextContent::new("hi"))],
        );
        assert_eq!(req.method(), STEERING_METHOD);
        let params = serde_json::to_value(&req).expect("serialize");
        assert_eq!(
            params,
            json!({
                "sessionId": "s-1",
                "prompt": [{"type": "text", "text": "hi"}],
                "_meta": {"steering": {"idleBehavior": "promptRequired"}}
            })
        );
    }

    #[test]
    fn known_outcomes_are_typed_and_others_are_errors() {
        let outcome = |o: &str| {
            serde_json::from_value::<SteeringResponse>(json!({"outcome": o, "reason": "x"}))
                .expect("parse")
                .into_outcome()
        };
        assert_eq!(outcome("injected"), Ok(SteerOutcome::Injected));
        assert_eq!(outcome("promptRequired"), Ok(SteerOutcome::PromptRequired));
        assert_eq!(outcome("startedNewTurn"), Ok(SteerOutcome::StartedNewTurn));
        assert!(matches!(
            outcome("failed"),
            Err(AgentError::Request { method, message })
                if method == STEERING_METHOD && message.contains("failed")
        ));
    }
}
