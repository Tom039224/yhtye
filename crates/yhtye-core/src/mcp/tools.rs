//! The Yhtye MCP tool catalog: names, roles, argument types and descriptions
//! (`docs/architecture/mcp-tools.md` §3–§4). Argument JSON schemas are generated
//! from the argument structs.

use std::sync::Arc;

use rmcp::model::{JsonObject, Tool};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::domain::{HelpKind, Role, StepSpec, TaskKind, ToolError, Verdict};

const ORCH: &[Role] = &[Role::Orchestrator];
const SUB: &[Role] = &[Role::Implementer, Role::Reviewer];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGroupArgs {
    /// Short title of the user's request.
    pub title: String,
    /// Optional longer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskArgs {
    /// Group id returned by create_group.
    pub group_id: String,
    /// Short display name of the task.
    pub title: String,
    /// `code` (edits files in its own worktree) or `investigate` (read-only).
    pub kind: TaskKind,
    /// Steps to run in order, e.g. [{"kind":"implement"}] or
    /// [{"kind":"implement"},{"kind":"review"}]. A final `done` is appended automatically.
    pub steps: Vec<StepSpec>,
    /// Task ids that must be done before this task starts.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// What the agent working on the task should do. May be omitted and given later
    /// with set_instruction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetInstructionArgs {
    pub task_id: String,
    /// Instruction for the task (must not be empty).
    pub instruction: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModifyStepsArgs {
    pub task_id: String,
    /// Replaces all steps that have not started yet.
    pub steps: Vec<StepSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointDecision {
    Continue,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveCheckpointArgs {
    pub task_id: String,
    pub decision: CheckpointDecision,
    /// Extra note passed to the agent of the next step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HelpAction {
    Resume,
    CancelTask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerHelpArgs {
    pub help_id: String,
    pub action: HelpAction,
    /// Reply sent to the agent that asked (required for `resume` of an agent's help).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelTaskArgs {
    pub task_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinishGroupArgs {
    pub group_id: String,
    /// Summary of the finished work for the user.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelGroupArgs {
    pub group_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatusArgs {
    /// Defaults to the active group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTaskArgs {
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportStepDoneArgs {
    /// What you did or found. Passed to the orchestrator and later steps.
    pub result: String,
    /// Required for reviewers (`approve` / `needs_changes`); not allowed for implementers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HelpArgs {
    /// `blocked` (cannot proceed), `question` (instruction unclear) or
    /// `policy` (want to change the approach).
    pub kind: HelpKind,
    /// What you need from the orchestrator.
    pub message: String,
}

/// Declares the tool catalog: `Variant => "name", Args, roles, "description"`.
macro_rules! tool_catalog {
    ($($variant:ident => $name:literal, $args:ty, $roles:expr, $desc:literal;)*) => {
        /// Name of a Yhtye MCP tool.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum ToolName { $($variant),* }

        /// A parsed tool call (typed arguments).
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(tag = "tool", content = "args", rename_all = "snake_case")]
        pub enum ToolCall { $($variant($args)),* }

        impl ToolName {
            pub const ALL: &'static [ToolName] = &[$(ToolName::$variant),*];

            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name),* }
            }

            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)* _ => None }
            }

            /// Roles allowed to call this tool.
            #[must_use]
            pub fn roles(self) -> &'static [Role] {
                match self { $(Self::$variant => $roles),* }
            }

            #[must_use]
            pub fn description(self) -> &'static str {
                match self { $(Self::$variant => $desc),* }
            }

            fn input_schema(self) -> Arc<JsonObject> {
                match self {
                    $(Self::$variant => rmcp::handler::server::common::schema_for_type::<$args>()),*
                }
            }

            fn parse_args(self, args: serde_json::Value) -> Result<ToolCall, serde_json::Error> {
                match self {
                    $(Self::$variant => serde_json::from_value(args).map(ToolCall::$variant)),*
                }
            }
        }

        impl ToolCall {
            #[must_use]
            pub fn name(&self) -> ToolName {
                match self { $(Self::$variant(_) => ToolName::$variant),* }
            }
        }
    };
}

tool_catalog! {
    CreateGroup => "create_group", CreateGroupArgs, ORCH,
        "Start a group of tasks for the user's request (at most one active group per project). Returns group_id for create_task.";
    CreateTask => "create_task", CreateTaskArgs, ORCH,
        "Create a task in a group. Yhtye starts a separate agent for it once its dependencies are done and it has an instruction, and runs its steps in order. Returns immediately; you are woken with [yhtye:...] messages when something needs you.";
    SetInstruction => "set_instruction", SetInstructionArgs, ORCH,
        "Give the instruction of a task that was created without one (or is awaiting one).";
    ModifySteps => "modify_steps", ModifyStepsArgs, ORCH,
        "Replace the steps of a task that have not started yet.";
    ResolveCheckpoint => "resolve_checkpoint", ResolveCheckpointArgs, ORCH,
        "Continue or abort a task waiting at a checkpoint step.";
    AnswerHelp => "answer_help", AnswerHelpArgs, ORCH,
        "Answer a help request: resume the task (reply is sent to the agent) or cancel it.";
    CancelTask => "cancel_task", CancelTaskArgs, ORCH,
        "Cancel a task and stop its agent.";
    FinishGroup => "finish_group", FinishGroupArgs, ORCH,
        "Finish a group whose tasks are all done or cancelled; merges its work into the base branch.";
    CancelGroup => "cancel_group", CancelGroupArgs, ORCH,
        "Cancel a group and all of its unfinished tasks.";
    GetStatus => "get_status", GetStatusArgs, ORCH,
        "Summary of a group (default: the active one) and its tasks.";
    GetTask => "get_task", GetTaskArgs, ORCH,
        "Details of one task: steps, results, open help.";
    ReportStepDone => "report_step_done", ReportStepDoneArgs, SUB,
        "Report that your current step is finished, with its result. Reviewers must give a verdict. After this call, end your turn.";
    Help => "help", HelpArgs, SUB,
        "Ask the orchestrator for help when blocked, when the instruction is unclear, or to change the approach. Then end your turn; the answer arrives as your next prompt.";
}

impl ToolName {
    #[must_use]
    pub fn allowed_for(self, role: Role) -> bool {
        self.roles().contains(&role)
    }

    /// The MCP tool definition.
    #[must_use]
    pub fn definition(self) -> Tool {
        Tool::new(self.as_str(), self.description(), self.input_schema())
    }
}

/// Tool definitions visible to `role` (`tools/list`).
#[must_use]
pub fn tools_for(role: Role) -> Vec<Tool> {
    ToolName::ALL
        .iter()
        .filter(|t| t.allowed_for(role))
        .map(|t| t.definition())
        .collect()
}

/// Parses the arguments of `tool`. Malformed arguments are an `invalid_argument`
/// tool error (not a protocol error) so the agent can read the message and retry.
pub fn parse_call(tool: ToolName, args: Option<JsonObject>) -> Result<ToolCall, ToolError> {
    let value = serde_json::Value::Object(args.unwrap_or_default());
    tool.parse_args(value).map_err(|e| {
        ToolError::invalid_argument(format!(
            "invalid arguments for {}: {e}. Check the tool's input schema.",
            tool.as_str()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ErrorCode, StepKind};
    use serde_json::json;

    fn obj(v: serde_json::Value) -> Option<JsonObject> {
        v.as_object().cloned()
    }

    #[test]
    fn names_round_trip_and_roles_partition() {
        for t in ToolName::ALL {
            assert_eq!(ToolName::from_name(t.as_str()), Some(*t));
        }
        let orch: Vec<_> = tools_for(Role::Orchestrator)
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(orch.len(), 11);
        assert!(orch.iter().any(|n| n == "create_task"));
        assert!(!orch.iter().any(|n| n == "report_step_done"));
        for role in [Role::Implementer, Role::Reviewer] {
            let names: Vec<_> = tools_for(role).into_iter().map(|t| t.name).collect();
            assert_eq!(names, vec!["report_step_done", "help"]);
        }
    }

    #[test]
    fn schemas_are_objects_with_required_fields() {
        let def = ToolName::CreateTask.definition();
        assert_eq!(def.input_schema.get("type"), Some(&json!("object")));
        let required = def.input_schema.get("required").expect("required");
        for f in ["group_id", "title", "kind", "steps"] {
            assert!(
                required.as_array().is_some_and(|r| r.contains(&json!(f))),
                "{f}"
            );
        }
    }

    #[test]
    fn parses_create_task_with_defaults() {
        let call = parse_call(
            ToolName::CreateTask,
            obj(json!({"group_id":"G-1","title":"t","kind":"code","steps":[{"kind":"implement"}]})),
        )
        .expect("parses");
        let ToolCall::CreateTask(args) = call else {
            panic!("wrong variant")
        };
        assert_eq!(args.steps, vec![StepSpec::new(StepKind::Implement)]);
        assert!(args.depends_on.is_empty());
        assert_eq!(args.instruction, None);
    }

    #[test]
    fn malformed_arguments_are_invalid_argument() {
        let cases = [
            (ToolName::CreateGroup, json!({})),
            (
                ToolName::CreateTask,
                json!({"group_id":"G-1","title":"t","kind":"docs","steps":[]}),
            ),
            (ToolName::Help, json!({"kind":"panic","message":"x"})),
            (ToolName::ReportStepDone, json!({"result":"x","extra":1})),
        ];
        for (tool, args) in cases {
            let err = parse_call(tool, obj(args)).expect_err("must fail");
            assert_eq!(err.code, ErrorCode::InvalidArgument);
            assert!(err.message.contains(tool.as_str()));
        }
    }
}
