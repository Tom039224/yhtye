//! The orchestration state machine (`docs/architecture/core-design.md` §5).
//!
//! Pure and I/O-free: [`decide`] takes a [`State`] and a [`DomainCommand`] and
//! returns the next state plus a [`Transition`] — the [`DomainEvent`]s that
//! happened (applied by the reducer [`State::apply`]), the [`Effect`]s the runtime
//! must carry out, and the tool reply.

mod agent_rules;
mod command;
mod event;
mod flow;
mod git_rules;
mod inbox;
mod machine;
mod restart;
mod state;
mod steps;
mod tools_orch;
mod tools_sub;
mod types;

pub use agent_rules::MAX_NUDGES;
pub use command::{
    AgentRef, DomainCommand, Effect, GitOp, GitResult, OrchestratorResume, Transition, TurnOutcome,
};
pub use event::DomainEvent;
pub use inbox::{InboxItem, InboxKind, render_batch};
pub use machine::{Machine, decide};
pub use state::{
    Counters, DEFAULT_MAX_REVIEW_ROUNDS, DomainConfig, Group, GroupStatus, Help, HelpSource,
    HelpState, InboxEntry, State, Step, StepStatus, Task, TaskStatus,
};
pub use steps::{normalize_steps, normalize_tail};
pub use types::{
    AgentHelpKind, ErrorCode, HelpKind, Role, StepKind, StepSpec, TaskKind, ToolError, Verdict,
};

#[cfg(test)]
mod tests;
