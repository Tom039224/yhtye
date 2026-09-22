//! Domain vocabulary and pure rules. Stage 2 has only the shared types and the
//! step-list rules; the full state machine arrives in Stage 3
//! (`docs/architecture/core-design.md` §5).

mod steps;
mod types;

pub use steps::normalize_steps;
pub use types::{ErrorCode, HelpKind, Role, StepKind, StepSpec, TaskKind, ToolError, Verdict};
