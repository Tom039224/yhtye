//! [`ToolPort`] that hands tool calls to the runtime loop, which applies them to
//! the state machine one at a time and answers when the call's effects are done.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::domain::ToolError;
use crate::mcp::{SessionBinding, ToolCall, ToolPort};

pub(super) struct ToolRequest {
    pub(super) binding: SessionBinding,
    pub(super) call: ToolCall,
    pub(super) reply: oneshot::Sender<Result<Value, ToolError>>,
}

#[derive(Clone)]
pub(super) struct LoopPort {
    tx: mpsc::UnboundedSender<ToolRequest>,
}

impl LoopPort {
    pub(super) fn new() -> (Self, mpsc::UnboundedReceiver<ToolRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

#[async_trait]
impl ToolPort for LoopPort {
    async fn call(&self, binding: SessionBinding, call: ToolCall) -> Result<Value, ToolError> {
        let (reply, rx) = oneshot::channel();
        let gone = || ToolError::internal("Yhtye's orchestration loop has stopped");
        self.tx
            .send(ToolRequest {
                binding,
                call,
                reply,
            })
            .map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }
}
