//! Stage 2 in-memory task board: a deliberately small stand-in for the Stage 3
//! state machine. It implements enough of `mcp-tools.md` to drive the
//! orchestrator ⇄ sub-agent loop (groups, tasks, sequential steps, help,
//! checkpoints) without git, persistence or review round insertion.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use super::inbox::{InboxItem, InboxKind};
use crate::domain::{
    HelpKind, Role, StepKind, StepSpec, TaskKind, ToolError, Verdict, normalize_steps,
};
use crate::mcp::SessionBinding;
use crate::mcp::tools::{
    AnswerHelpArgs, CancelTaskArgs, CheckpointDecision, CreateGroupArgs, CreateTaskArgs,
    FinishGroupArgs, GetStatusArgs, GetTaskArgs, HelpAction, HelpArgs, ReportStepDoneArgs,
    ResolveCheckpointArgs, SetInstructionArgs, ToolCall,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    AwaitingInstruction,
    Running,
    Checkpoint,
    Handling,
    Done,
    Cancelled,
}

impl TaskStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AwaitingInstruction => "awaiting_instruction",
            Self::Running => "running",
            Self::Checkpoint => "checkpoint",
            Self::Handling => "handling",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub kind: StepKind,
    pub instruction: Option<String>,
    pub status: StepStatus,
    pub result: Option<String>,
    pub verdict: Option<Verdict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub group: String,
    pub title: String,
    pub kind: TaskKind,
    pub depends_on: Vec<String>,
    pub instruction: Option<String>,
    pub steps: Vec<Step>,
    pub current: usize,
    pub status: TaskStatus,
    /// `report_step_done` was called; advance when the agent's turn ends.
    #[serde(skip)]
    awaiting_advance: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupStatus {
    Active,
    Done,
}

#[derive(Debug, Clone, Serialize)]
struct Group {
    id: String,
    title: String,
    status: GroupStatus,
}

#[derive(Debug, Clone, Serialize)]
struct Help {
    id: String,
    task: String,
    kind: HelpKind,
    message: String,
    answered: bool,
}

/// What the runtime must do after a board change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Run step `step` of `task` with an agent of `role`.
    StartStep(StepStart),
    /// Queue an inbox item for the orchestrator.
    Wake(InboxItem),
    /// Send `text` as the next prompt to the agent working on `task`.
    PromptTask { task: String, text: String },
    /// The task reached a terminal state; stop its sessions.
    TaskFinished { task: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepStart {
    pub group: String,
    pub task: String,
    pub title: String,
    pub kind: TaskKind,
    pub step: usize,
    pub step_count: usize,
    pub step_kind: StepKind,
    pub role: Role,
    pub instruction: String,
}

pub type Outcome = Result<(Value, Vec<Effect>), ToolError>;

#[derive(Debug, Default)]
pub struct Board {
    base_branch: String,
    groups: Vec<Group>,
    tasks: BTreeMap<String, Task>,
    helps: BTreeMap<String, Help>,
    next_task: u32,
    next_help: u32,
}

fn non_empty(field: &str, value: &str) -> Result<(), ToolError> {
    if value.trim().is_empty() {
        return Err(ToolError::invalid_argument(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

impl Board {
    #[must_use]
    pub fn new(base_branch: impl Into<String>) -> Self {
        Self {
            base_branch: base_branch.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    /// Applies one tool call from the session described by `binding`.
    pub fn apply(&mut self, binding: &SessionBinding, call: ToolCall) -> Outcome {
        match call {
            ToolCall::CreateGroup(a) => self.create_group(a),
            ToolCall::CreateTask(a) => self.create_task(a),
            ToolCall::SetInstruction(a) => self.set_instruction(a),
            ToolCall::ResolveCheckpoint(a) => self.resolve_checkpoint(a),
            ToolCall::AnswerHelp(a) => self.answer_help(a),
            ToolCall::CancelTask(a) => self.cancel_task(&a),
            ToolCall::FinishGroup(a) => self.finish_group(&a),
            ToolCall::GetStatus(a) => self.get_status(&a),
            ToolCall::GetTask(a) => self.get_task(&a),
            ToolCall::ReportStepDone(a) => self.report_step_done(binding, a),
            ToolCall::Help(a) => self.help(binding, a),
            ToolCall::ModifySteps(_) | ToolCall::CancelGroup(_) => Err(ToolError::internal(
                "not implemented yet in this Yhtye build (Stage 2 in-memory board)",
            )),
        }
    }

    /// The agent of `task` ended its turn: start the next step if it reported.
    pub fn on_turn_ended(&mut self, task: &str) -> Vec<Effect> {
        let Some(t) = self.tasks.get_mut(task) else {
            return Vec::new();
        };
        if !t.awaiting_advance || t.status != TaskStatus::Running {
            return Vec::new();
        }
        t.awaiting_advance = false;
        t.current += 1;
        self.start_current_step(task)
    }

    fn active_group(&self) -> Option<&Group> {
        self.groups.iter().find(|g| g.status == GroupStatus::Active)
    }

    fn group(&self, id: &str) -> Result<&Group, ToolError> {
        self.groups
            .iter()
            .find(|g| g.id == id)
            .ok_or_else(|| ToolError::not_found(format!("no group {id}")))
    }

    fn task_mut(&mut self, id: &str) -> Result<&mut Task, ToolError> {
        self.tasks
            .get_mut(id)
            .ok_or_else(|| ToolError::not_found(format!("no task {id}")))
    }

    fn create_group(&mut self, a: CreateGroupArgs) -> Outcome {
        non_empty("title", &a.title)?;
        if let Some(g) = self.active_group() {
            return Err(ToolError::conflict(format!(
                "group {} is still active; finish_group or cancel_group it first",
                g.id
            )));
        }
        let id = format!("G-{}", self.groups.len() + 1);
        self.groups.push(Group {
            id: id.clone(),
            title: a.title,
            status: GroupStatus::Active,
        });
        let reply = json!({
            "group_id": id,
            "group_branch": format!("yhtye/{id}"),
            "base_branch": self.base_branch,
        });
        Ok((reply, Vec::new()))
    }

    fn create_task(&mut self, a: CreateTaskArgs) -> Outcome {
        non_empty("title", &a.title)?;
        let group = self.group(&a.group_id)?;
        if group.status != GroupStatus::Active {
            return Err(ToolError::invalid_state(format!(
                "group {} is {:?}, not active",
                group.id, group.status
            )));
        }
        let steps = normalize_steps(a.kind, &a.steps)?;
        for dep in &a.depends_on {
            match self.tasks.get(dep) {
                Some(t) if t.group == a.group_id => {}
                _ => {
                    return Err(ToolError::not_found(format!(
                        "depends_on: no task {dep} in group {}",
                        a.group_id
                    )));
                }
            }
        }
        self.next_task += 1;
        let id = format!("T-{}", self.next_task);
        let task = Task {
            id: id.clone(),
            group: a.group_id,
            title: a.title,
            kind: a.kind,
            depends_on: a.depends_on,
            instruction: a.instruction.filter(|i| !i.trim().is_empty()),
            steps: steps.iter().map(new_step).collect(),
            current: 0,
            status: TaskStatus::Pending,
            awaiting_advance: false,
        };
        self.tasks.insert(id.clone(), task);
        let effects = self.try_start(&id);
        let status = self.tasks.get(&id).map(|t| t.status);
        Ok((
            json!({ "task_id": id, "status": status, "steps": steps }),
            effects,
        ))
    }

    fn set_instruction(&mut self, a: SetInstructionArgs) -> Outcome {
        non_empty("instruction", &a.instruction)?;
        let t = self.task_mut(&a.task_id)?;
        if !matches!(
            t.status,
            TaskStatus::Pending | TaskStatus::AwaitingInstruction
        ) {
            return Err(ToolError::invalid_state(format!(
                "task {} is {}; the instruction can only be set before it starts (use answer_help or modify_steps)",
                t.id,
                t.status.as_str()
            )));
        }
        t.instruction = Some(a.instruction);
        let effects = self.try_start(&a.task_id);
        Ok((self.task_status_reply(&a.task_id), effects))
    }

    fn resolve_checkpoint(&mut self, a: ResolveCheckpointArgs) -> Outcome {
        let t = self.task_mut(&a.task_id)?;
        if t.status != TaskStatus::Checkpoint {
            return Err(ToolError::invalid_state(format!(
                "task {} is {}, not waiting at a checkpoint",
                t.id,
                t.status.as_str()
            )));
        }
        let effects = match a.decision {
            CheckpointDecision::Continue => {
                if let Some(step) = t.steps.get_mut(t.current) {
                    step.status = StepStatus::Done;
                    step.result = a.note;
                }
                t.current += 1;
                self.start_current_step(&a.task_id)
            }
            CheckpointDecision::Abort => self.finish_task(&a.task_id, TaskStatus::Cancelled),
        };
        let t = self.task_mut(&a.task_id)?;
        let reply = json!({ "task_id": t.id, "status": t.status, "current_step": t.current });
        Ok((reply, effects))
    }

    fn answer_help(&mut self, a: AnswerHelpArgs) -> Outcome {
        let help = self
            .helps
            .get_mut(&a.help_id)
            .ok_or_else(|| ToolError::not_found(format!("no help {}", a.help_id)))?;
        if help.answered {
            return Err(ToolError::invalid_state(format!(
                "help {} was already answered",
                help.id
            )));
        }
        let task = help.task.clone();
        let effects = match a.action {
            HelpAction::Resume => {
                let reply = a.reply.filter(|r| !r.trim().is_empty()).ok_or_else(|| {
                    ToolError::invalid_argument(
                        "reply is required to resume an agent's help request",
                    )
                })?;
                help.answered = true;
                let text = format!("[yhtye:help_answer] help_id={}\n{reply}", help.id);
                self.task_mut(&task)?.status = TaskStatus::Running;
                vec![Effect::PromptTask {
                    task: task.clone(),
                    text,
                }]
            }
            HelpAction::CancelTask => {
                help.answered = true;
                self.finish_task(&task, TaskStatus::Cancelled)
            }
        };
        let status = self.task_mut(&task)?.status;
        Ok((
            json!({ "help_id": a.help_id, "task_id": task, "task_status": status }),
            effects,
        ))
    }

    fn cancel_task(&mut self, a: &CancelTaskArgs) -> Outcome {
        let t = self.task_mut(&a.task_id)?;
        if t.status.is_terminal() {
            return Err(ToolError::invalid_state(format!(
                "task {} is already {}",
                t.id,
                t.status.as_str()
            )));
        }
        let effects = self.finish_task(&a.task_id, TaskStatus::Cancelled);
        Ok((
            json!({ "task_id": a.task_id, "status": "cancelled" }),
            effects,
        ))
    }

    fn finish_group(&mut self, a: &FinishGroupArgs) -> Outcome {
        self.group(&a.group_id)?;
        let open: Vec<String> = self
            .tasks_of(&a.group_id)
            .filter(|t| !t.status.is_terminal())
            .map(|t| format!("{} ({})", t.id, t.status.as_str()))
            .collect();
        if !open.is_empty() {
            return Err(ToolError::invalid_state(format!(
                "tasks not finished yet: {}",
                open.join(", ")
            )));
        }
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == a.group_id) {
            g.status = GroupStatus::Done;
        }
        let reply = json!({
            "group_id": a.group_id,
            "status": "done",
            "merge": { "ok": true, "detail": "no git merge in this build (Stage 2 in-memory board)" },
        });
        Ok((reply, Vec::new()))
    }

    fn get_status(&self, a: &GetStatusArgs) -> Outcome {
        let group = match &a.group_id {
            Some(id) => self.group(id)?,
            None => self
                .active_group()
                .ok_or_else(|| ToolError::not_found("there is no active group"))?,
        };
        let tasks: Vec<Value> = self
            .tasks_of(&group.id)
            .map(|t| {
                json!({
                    "task_id": t.id, "title": t.title, "kind": t.kind, "status": t.status,
                    "depends_on": t.depends_on,
                    "current_step": t.steps.get(t.current).map(|s| s.kind),
                    "steps_summary": t.steps.iter().map(|s| format!("{:?}:{:?}", s.kind, s.status)).collect::<Vec<_>>(),
                })
            })
            .collect();
        let open_helps: Vec<&Help> = self.helps.values().filter(|h| !h.answered).collect();
        let reply = json!({
            "group": { "group_id": group.id, "title": group.title, "status": group.status },
            "tasks": tasks,
            "open_helps": open_helps,
        });
        Ok((reply, Vec::new()))
    }

    fn get_task(&self, a: &GetTaskArgs) -> Outcome {
        let t = self
            .tasks
            .get(&a.task_id)
            .ok_or_else(|| ToolError::not_found(format!("no task {}", a.task_id)))?;
        let helps: Vec<&Help> = self
            .helps
            .values()
            .filter(|h| h.task == t.id && !h.answered)
            .collect();
        let mut reply = serde_json::to_value(t).map_err(|e| ToolError::internal(e.to_string()))?;
        reply["open_helps"] = json!(helps);
        Ok((reply, Vec::new()))
    }

    fn report_step_done(&mut self, b: &SessionBinding, a: ReportStepDoneArgs) -> Outcome {
        non_empty("result", &a.result)?;
        match (b.role, a.verdict) {
            (Role::Reviewer, None) => {
                return Err(ToolError::invalid_argument(
                    "reviewers must give a verdict: approve or needs_changes",
                ));
            }
            (Role::Implementer, Some(_)) => {
                return Err(ToolError::invalid_argument(
                    "implementers must not give a verdict; omit it",
                ));
            }
            _ => {}
        }
        let t = self.bound_task(b)?;
        let step = t
            .steps
            .get_mut(t.current)
            .filter(|s| s.status == StepStatus::Running);
        let Some(step) = step.filter(|_| b.step == Some(t.current)) else {
            return Err(ToolError::invalid_state(format!(
                "your step is not the running step of task {} (status {}, current step {})",
                t.id,
                t.status.as_str(),
                t.current
            )));
        };
        step.status = StepStatus::Done;
        step.result = Some(a.result);
        step.verdict = a.verdict;
        t.awaiting_advance = true;
        Ok((json!({ "ok": true, "next": "end_turn" }), Vec::new()))
    }

    fn help(&mut self, b: &SessionBinding, a: HelpArgs) -> Outcome {
        non_empty("message", &a.message)?;
        let task_id = self.bound_task(b)?.id.clone();
        if let Some(h) = self
            .helps
            .values()
            .find(|h| h.task == task_id && !h.answered)
        {
            return Err(ToolError::invalid_state(format!(
                "help {} for this task is still waiting for an answer",
                h.id
            )));
        }
        self.next_help += 1;
        let id = format!("H-{}", self.next_help);
        let item = InboxItem::new(
            InboxKind::HelpRaised,
            &[
                ("help_id", &id),
                ("task", &task_id),
                ("kind", a.kind.as_str()),
            ],
            a.message.clone(),
        );
        self.helps.insert(
            id.clone(),
            Help {
                id: id.clone(),
                task: task_id.clone(),
                kind: a.kind,
                message: a.message,
                answered: false,
            },
        );
        self.task_mut(&task_id)?.status = TaskStatus::Handling;
        let reply = json!({ "help_id": id, "next": "end_turn_and_wait" });
        Ok((reply, vec![Effect::Wake(item)]))
    }

    fn bound_task(&mut self, b: &SessionBinding) -> Result<&mut Task, ToolError> {
        let id = b
            .task
            .as_deref()
            .ok_or_else(|| ToolError::forbidden("this session is not bound to a task"))?;
        self.task_mut(id)
    }

    fn task_status_reply(&self, id: &str) -> Value {
        json!({ "task_id": id, "status": self.tasks.get(id).map(|t| t.status) })
    }

    fn tasks_of<'a>(&'a self, group: &'a str) -> impl Iterator<Item = &'a Task> + 'a {
        self.tasks.values().filter(move |t| t.group == group)
    }

    /// Starts a pending task whose dependencies are done (or asks for its instruction).
    fn try_start(&mut self, id: &str) -> Vec<Effect> {
        let Some(t) = self.tasks.get(id) else {
            return Vec::new();
        };
        if !matches!(
            t.status,
            TaskStatus::Pending | TaskStatus::AwaitingInstruction
        ) {
            return Vec::new();
        }
        let deps_done = t.depends_on.iter().all(|d| {
            self.tasks
                .get(d)
                .is_some_and(|d| d.status == TaskStatus::Done)
        });
        if !deps_done {
            return Vec::new();
        }
        if t.instruction.is_some() {
            return self.start_current_step(id);
        }
        if t.status == TaskStatus::AwaitingInstruction {
            return Vec::new();
        }
        let body = t
            .depends_on
            .iter()
            .filter_map(|d| self.tasks.get(d))
            .map(|d| format!("{}: {}", d.id, final_result(d)))
            .collect::<Vec<_>>()
            .join("\n");
        let item = InboxItem::new(InboxKind::InstructionNeeded, &[("task", id)], body);
        if let Some(t) = self.tasks.get_mut(id) {
            t.status = TaskStatus::AwaitingInstruction;
        }
        vec![Effect::Wake(item)]
    }

    fn start_current_step(&mut self, id: &str) -> Vec<Effect> {
        let Some(t) = self.tasks.get_mut(id) else {
            return Vec::new();
        };
        let count = t.steps.len();
        let Some(step) = t.steps.get_mut(t.current) else {
            return self.finish_task(id, TaskStatus::Done);
        };
        match step.kind {
            StepKind::Done => {
                step.status = StepStatus::Done;
                self.finish_task(id, TaskStatus::Done)
            }
            StepKind::Checkpoint => {
                step.status = StepStatus::Running;
                t.status = TaskStatus::Checkpoint;
                let body = previous_results(t);
                let step_no = t.current.to_string();
                let item = InboxItem::new(
                    InboxKind::CheckpointReached,
                    &[("task", id), ("step", &step_no)],
                    body,
                );
                vec![Effect::Wake(item)]
            }
            kind @ (StepKind::Implement | StepKind::Review) => {
                step.status = StepStatus::Running;
                let instruction = step
                    .instruction
                    .clone()
                    .or_else(|| t.instruction.clone())
                    .unwrap_or_default();
                t.status = TaskStatus::Running;
                let role = kind.role().unwrap_or(Role::Implementer);
                vec![Effect::StartStep(StepStart {
                    group: t.group.clone(),
                    task: t.id.clone(),
                    title: t.title.clone(),
                    kind: t.kind,
                    step: t.current,
                    step_count: count,
                    step_kind: kind,
                    role,
                    instruction,
                })]
            }
        }
    }

    /// Moves a task to a terminal state, then starts dependents and checks the group.
    fn finish_task(&mut self, id: &str, status: TaskStatus) -> Vec<Effect> {
        let Some(t) = self.tasks.get_mut(id) else {
            return Vec::new();
        };
        t.status = status;
        t.awaiting_advance = false;
        let group = t.group.clone();
        let mut effects = vec![Effect::TaskFinished {
            task: id.to_string(),
        }];
        let pending: Vec<String> = self
            .tasks_of(&group)
            .filter(|t| t.status == TaskStatus::Pending)
            .map(|t| t.id.clone())
            .collect();
        for p in pending {
            effects.extend(self.try_start(&p));
        }
        effects.extend(self.settle_check(&group));
        effects
    }

    fn settle_check(&self, group: &str) -> Option<Effect> {
        let mut tasks = self.tasks_of(group).peekable();
        tasks.peek()?;
        let mut lines = Vec::new();
        for t in tasks {
            if !t.status.is_terminal() {
                // Pending tasks blocked by a cancelled dependency can never start.
                let blocked = t.status == TaskStatus::Pending
                    && t.depends_on.iter().any(|d| {
                        self.tasks
                            .get(d)
                            .is_some_and(|d| d.status == TaskStatus::Cancelled)
                    });
                if !blocked {
                    return None;
                }
                lines.push(format!(
                    "{} cannot start (a dependency was cancelled)",
                    t.id
                ));
                continue;
            }
            lines.push(format!(
                "{} {}: {}",
                t.id,
                t.status.as_str(),
                final_result(t)
            ));
        }
        Some(Effect::Wake(InboxItem::new(
            InboxKind::GroupSettled,
            &[("group", group)],
            lines.join("\n"),
        )))
    }
}

fn new_step(spec: &StepSpec) -> Step {
    Step {
        kind: spec.kind,
        instruction: spec.instruction.clone(),
        status: StepStatus::Pending,
        result: None,
        verdict: None,
    }
}

fn final_result(t: &Task) -> String {
    t.steps
        .iter()
        .rev()
        .find_map(|s| s.result.clone())
        .unwrap_or_else(|| "(no result)".into())
}

fn previous_results(t: &Task) -> String {
    t.steps[..t.current]
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.result
                .as_ref()
                .map(|r| format!("step {i} ({:?}): {r}", s.kind))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
