//! [`ToolPort`] over the Stage 2 in-memory [`Board`]. Board effects are sent to the
//! orchestration loop through a channel so tool calls return immediately.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::board::{Board, Effect};
use crate::domain::ToolError;
use crate::mcp::{SessionBinding, ToolCall, ToolPort};

/// Cheap to clone; all clones share one board.
#[derive(Clone)]
pub struct MemoryToolPort {
    board: Arc<Mutex<Board>>,
    effects: mpsc::UnboundedSender<Effect>,
}

impl MemoryToolPort {
    /// A port over a fresh board; effects are delivered to the returned receiver.
    #[must_use]
    pub fn new(base_branch: &str) -> (Self, mpsc::UnboundedReceiver<Effect>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let port = Self {
            board: Arc::new(Mutex::new(Board::new(base_branch))),
            effects: tx,
        };
        (port, rx)
    }

    /// The agent working on `task` ended its turn (see [`Board::on_turn_ended`]).
    pub fn on_turn_ended(&self, task: &str) {
        let effects = self.lock().on_turn_ended(task);
        self.emit(effects);
    }

    /// Runs `f` with read access to the board.
    pub fn with_board<T>(&self, f: impl FnOnce(&Board) -> T) -> T {
        f(&self.lock())
    }

    fn lock(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn emit(&self, effects: Vec<Effect>) {
        for e in effects {
            if self.effects.send(e).is_err() {
                tracing::warn!("orchestration loop is gone; dropping board effect");
            }
        }
    }
}

#[async_trait]
impl ToolPort for MemoryToolPort {
    async fn call(
        &self,
        binding: SessionBinding,
        call: ToolCall,
    ) -> Result<serde_json::Value, ToolError> {
        // Effects are queued before the tool returns, so the loop sees them
        // before the caller's turn can end.
        let (reply, effects) = self.lock().apply(&binding, call)?;
        self.emit(effects);
        Ok(reply)
    }
}
