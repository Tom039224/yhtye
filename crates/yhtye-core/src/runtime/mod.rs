//! Multi-session runtime. Stage 2: an in-memory tool port ([`MemoryToolPort`]) and
//! the orchestration loop that starts sub-agent sessions for tasks and wakes the
//! orchestrator. Stage 3 replaces the board with the persistent state machine.

mod board;
mod driver;
mod inbox;
mod memory_port;
mod orchestration;

pub use board::{Board, Effect, StepStart, Task, TaskStatus};
pub use inbox::{InboxItem, InboxKind, render_batch};
pub use memory_port::MemoryToolPort;
pub use orchestration::{
    ORCHESTRATOR_SESSION, OrchError, OrchEvent, Orchestration, OrchestrationConfig,
};
