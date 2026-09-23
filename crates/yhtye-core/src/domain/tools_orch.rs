//! Orchestrator tools that change state (`mcp-tools.md` §3).

use serde_json::{Value, json};

use super::command::{Effect, GitOp, MergeTrigger};
use super::event::DomainEvent;
use super::flow::help_agent;
use super::machine::{Tx, non_empty};
use super::state::{Group, GroupStatus, HelpSource, Step, Task, TaskStatus};
use super::steps::{normalize_steps, normalize_tail};
use super::types::{StepKind, ToolError};
use crate::agents::AgentChoice;
use crate::mcp::SessionBinding;
use crate::mcp::tools::{
    AnswerHelpArgs, CancelGroupArgs, CancelTaskArgs, CheckpointDecision, CreateGroupArgs,
    CreateTaskArgs, FinishGroupArgs, HelpAction, ModifyStepsArgs, ResolveCheckpointArgs,
    SetInstructionArgs, ToolCall,
};

type Reply = Result<Value, ToolError>;

impl Tx {
    /// Dispatches a tool call (everything except `create_group`).
    pub(super) fn tool(
        &mut self,
        binding: &SessionBinding,
        call: ToolCall,
    ) -> Result<(), ToolError> {
        let reply = match call {
            ToolCall::CreateGroup(_) => Err(ToolError::internal(
                "create_group must be sent as DomainCommand::CreateGroup",
            )),
            ToolCall::CreateTask(a) => self.create_task(a),
            ToolCall::SetInstruction(a) => self.set_instruction(a),
            ToolCall::ModifySteps(a) => self.modify_steps(a),
            ToolCall::ResolveCheckpoint(a) => self.resolve_checkpoint(a),
            ToolCall::AnswerHelp(a) => self.answer_help(a),
            ToolCall::CancelTask(a) => self.cancel_task_tool(&a),
            ToolCall::FinishGroup(a) => self.finish_group(a),
            ToolCall::CancelGroup(a) => self.cancel_group(&a),
            ToolCall::GetStatus(a) => self.get_status(&a),
            ToolCall::GetTask(a) => self.get_task(&a),
            ToolCall::ReportStepDone(a) => self.report_step_done(binding, a),
            ToolCall::Help(a) => self.help(binding, a),
        }?;
        self.reply(Ok(reply));
        Ok(())
    }

    pub(super) fn create_group(
        &mut self,
        a: CreateGroupArgs,
        base_branch: Option<String>,
    ) -> Result<(), ToolError> {
        non_empty("title", &a.title)?;
        if let Some(g) = self.state.open_group() {
            return Err(ToolError::conflict(format!(
                "group {} is still {}; finish_group or cancel_group it first",
                g.id,
                g.status.as_str()
            )));
        }
        let base_branch = base_branch.ok_or_else(|| {
            ToolError::invalid_state(
                "the project's main worktree is not on a branch (detached HEAD?), so there is \
                 no base branch; ask the user to check out a branch",
            )
        })?;
        let id = format!("G-{}", self.state.counters.groups + 1);
        let group_branch = super::state::group_branch(&id);
        self.emit(DomainEvent::GroupCreated {
            group: Group {
                id: id.clone(),
                title: a.title,
                summary: a.summary,
                base_branch: base_branch.clone(),
                group_branch: group_branch.clone(),
                status: GroupStatus::Active,
                finish_summary: None,
                finish_nudges: 0,
                detail: None,
            },
        });
        self.effect(Effect::Git(GitOp::CreateGroupBranch {
            group: id.clone(),
            group_branch: group_branch.clone(),
            base_branch: base_branch.clone(),
        }));
        let reply =
            json!({ "group_id": id, "group_branch": group_branch, "base_branch": base_branch });
        self.reply(Ok(reply));
        Ok(())
    }

    fn create_task(&mut self, a: CreateTaskArgs) -> Reply {
        non_empty("title", &a.title)?;
        let group = self
            .state
            .group(&a.group_id)
            .ok_or_else(|| ToolError::not_found(format!("no group {}", a.group_id)))?;
        if group.status != GroupStatus::Active {
            return Err(ToolError::invalid_state(format!(
                "group {} is {}, not active",
                group.id,
                group.status.as_str()
            )));
        }
        let steps = normalize_steps(a.kind, &a.steps)?;
        let depends_on = self.check_dependencies(&a.group_id, &a.depends_on)?;
        let id = format!("T-{}", self.state.counters.tasks + 1);
        self.emit(DomainEvent::TaskCreated {
            task: Task {
                id: id.clone(),
                group: a.group_id,
                title: a.title,
                kind: a.kind,
                depends_on,
                instruction: a.instruction.filter(|i| !i.trim().is_empty()),
                steps: steps.iter().map(Step::from_spec).collect(),
                current: 0,
                status: TaskStatus::Pending,
                review_rounds: 0,
                workdir: None,
                cancel_reason: None,
                // Checked against the role's candidates by the runtime, which
                // passes the resolved choice (`core-design.md` §15.5).
                agent: a.harness.map(|harness| AgentChoice {
                    harness,
                    model: a.model,
                }),
                review_agent: a.review_harness.map(|harness| AgentChoice {
                    harness,
                    model: a.review_model,
                }),
            },
        });
        self.try_start(&id);
        let status = self.task(&id)?.status;
        Ok(json!({ "task_id": id, "status": status, "steps": steps }))
    }

    /// Dependencies must be tasks of the same group that are not cancelled.
    /// (A new task can only depend on existing ones, so cycles cannot occur.)
    fn check_dependencies(&self, group: &str, deps: &[String]) -> Result<Vec<String>, ToolError> {
        let mut out: Vec<String> = Vec::new();
        for dep in deps {
            let t = self
                .state
                .task(dep)
                .filter(|t| t.group == group)
                .ok_or_else(|| {
                    ToolError::not_found(format!("depends_on: no task {dep} in group {group}"))
                })?;
            if t.status == TaskStatus::Cancelled || self.state.is_blocked(t) {
                return Err(ToolError::invalid_state(format!(
                    "depends_on: task {dep} is cancelled (or can never start), so this task could never start"
                )));
            }
            if !out.contains(dep) {
                out.push(dep.clone());
            }
        }
        Ok(out)
    }

    fn set_instruction(&mut self, a: SetInstructionArgs) -> Reply {
        non_empty("instruction", &a.instruction)?;
        let t = self.task(&a.task_id)?;
        if !matches!(
            t.status,
            TaskStatus::Pending | TaskStatus::AwaitingInstruction
        ) {
            return Err(ToolError::invalid_state(format!(
                "task {} is {}; the instruction can only be set before it starts \
                 (use answer_help or modify_steps)",
                t.id,
                t.status.as_str()
            )));
        }
        self.emit(DomainEvent::InstructionSet {
            task: a.task_id.clone(),
            instruction: a.instruction,
        });
        self.try_start(&a.task_id);
        let status = self.task(&a.task_id)?.status;
        Ok(json!({ "task_id": a.task_id, "status": status }))
    }

    fn modify_steps(&mut self, a: ModifyStepsArgs) -> Reply {
        let t = self.task(&a.task_id)?;
        if t.status.is_terminal() || t.status == TaskStatus::Merging {
            return Err(ToolError::invalid_state(format!(
                "task {} is {}; its steps can no longer change",
                t.id,
                t.status.as_str()
            )));
        }
        let steps = normalize_tail(t.kind, &a.steps)?;
        let from = t.pending_tail();
        if t.steps[..from].iter().any(|s| s.kind == StepKind::Done) {
            return Err(ToolError::invalid_state(format!(
                "task {} has already started its final `done` step",
                t.id
            )));
        }
        self.emit(DomainEvent::StepsReplaced {
            task: a.task_id.clone(),
            from,
            steps,
        });
        let t = self.task(&a.task_id)?;
        Ok(json!({ "task_id": t.id, "steps": step_list(t) }))
    }

    fn resolve_checkpoint(&mut self, a: ResolveCheckpointArgs) -> Reply {
        let t = self.task(&a.task_id)?;
        if t.status != TaskStatus::Checkpoint {
            return Err(ToolError::invalid_state(format!(
                "task {} is {}, not waiting at a checkpoint",
                t.id,
                t.status.as_str()
            )));
        }
        let index = t.current;
        let note = a.note.filter(|n| !n.trim().is_empty());
        match a.decision {
            CheckpointDecision::Continue => {
                self.emit(DomainEvent::StepCompleted {
                    task: a.task_id.clone(),
                    step: index,
                    result: note.clone().unwrap_or_else(|| "continue".into()),
                    verdict: None,
                });
                self.advance_with_note(&a.task_id, note);
            }
            CheckpointDecision::Abort => {
                let reason = note.map_or_else(
                    || "aborted at checkpoint".to_string(),
                    |n| format!("aborted at checkpoint: {n}"),
                );
                self.cancel_task(&a.task_id, &reason);
            }
        }
        let t = self.task(&a.task_id)?;
        Ok(json!({ "task_id": t.id, "status": t.status, "current_step": t.current }))
    }

    fn advance_with_note(&mut self, id: &str, note: Option<String>) {
        if let Some(next) = self.state.task(id).map(|t| t.current + 1) {
            self.start_step(id, next, note);
        }
    }

    fn answer_help(&mut self, a: AnswerHelpArgs) -> Reply {
        let help = self
            .state
            .help(&a.help_id)
            .ok_or_else(|| ToolError::not_found(format!("no help {}", a.help_id)))?
            .clone();
        if !help.is_open() {
            return Err(ToolError::invalid_state(format!(
                "help {} is already {:?}",
                help.id, help.state
            )));
        }
        let reply = a.reply.filter(|r| !r.trim().is_empty());
        let resume_agent = help_agent(&help).filter(|_| !help.agent_lost);
        if a.action == HelpAction::Resume && resume_agent.is_some() && reply.is_none() {
            return Err(ToolError::invalid_argument(
                "reply is required to resume an agent's help request (it becomes the agent's next prompt)",
            ));
        }
        self.emit(DomainEvent::HelpAnswered {
            help: help.id.clone(),
            action: a.action,
            reply: reply.clone(),
        });
        match (a.action, resume_agent, reply) {
            (HelpAction::CancelTask, ..) => {
                self.cancel_task(&help.task, &format!("cancelled on help {}", help.id));
            }
            (HelpAction::Resume, Some(agent), Some(text)) => {
                self.set_status(&help.task, TaskStatus::Running);
                let text = format!("[yhtye:help_answer] help_id={}\n{text}", help.id);
                self.effect(Effect::PromptAgent { agent, text });
            }
            (HelpAction::Resume, _, note) => self.begin_task(&help.task, note),
        }
        let status = self.task(&help.task)?.status;
        Ok(json!({ "help_id": help.id, "task_id": help.task, "task_status": status }))
    }

    fn cancel_task_tool(&mut self, a: &CancelTaskArgs) -> Reply {
        let t = self.task(&a.task_id)?;
        if t.status.is_terminal() {
            return Err(ToolError::invalid_state(format!(
                "task {} is already {}",
                t.id,
                t.status.as_str()
            )));
        }
        self.cancel_task(&a.task_id, &a.reason);
        Ok(json!({ "task_id": a.task_id, "status": "cancelled" }))
    }

    fn finish_group(&mut self, a: FinishGroupArgs) -> Reply {
        let g = self
            .state
            .group(&a.group_id)
            .ok_or_else(|| ToolError::not_found(format!("no group {}", a.group_id)))?;
        if g.status != GroupStatus::Active {
            return Err(ToolError::invalid_state(format!(
                "group {} is {}, not active",
                g.id,
                g.status.as_str()
            )));
        }
        let open: Vec<String> = self
            .state
            .tasks_of(&a.group_id)
            .filter(|t| !t.status.is_terminal())
            .map(|t| format!("{} ({})", t.id, t.status.as_str()))
            .collect();
        if !open.is_empty() {
            return Err(ToolError::invalid_state(format!(
                "tasks not finished yet: {} (wait for them, or cancel_task them)",
                open.join(", ")
            )));
        }
        let op = GitOp::MergeGroup {
            group: g.id.clone(),
            group_branch: g.group_branch.clone(),
            base_branch: g.base_branch.clone(),
            trigger: MergeTrigger::FinishGroup,
        };
        self.emit(DomainEvent::GroupFinishing {
            group: a.group_id.clone(),
            summary: a.summary,
        });
        self.effect(Effect::Git(op));
        Ok(json!({ "group_id": a.group_id, "status": GroupStatus::Finishing }))
    }

    fn cancel_group(&mut self, a: &CancelGroupArgs) -> Reply {
        let g = self
            .state
            .group(&a.group_id)
            .ok_or_else(|| ToolError::not_found(format!("no group {}", a.group_id)))?;
        if !matches!(g.status, GroupStatus::Active | GroupStatus::MergeBlocked) {
            return Err(ToolError::invalid_state(format!(
                "group {} is {}; only active or merge_blocked groups can be cancelled",
                g.id,
                g.status.as_str()
            )));
        }
        // Mark the group first so the task cancellations do not report it settled.
        self.emit(DomainEvent::GroupCancelled {
            group: a.group_id.clone(),
            reason: a.reason.clone(),
        });
        let open: Vec<String> = self
            .state
            .tasks_of(&a.group_id)
            .filter(|t| !t.status.is_terminal())
            .map(|t| t.id.clone())
            .collect();
        for id in open {
            self.cancel_task(&id, &format!("group cancelled: {}", a.reason));
        }
        // After the tasks' own worktrees (their effects run first).
        self.effect(Effect::Git(GitOp::RemoveGroupWorkspace {
            group: a.group_id.clone(),
        }));
        Ok(json!({ "group_id": a.group_id, "status": "cancelled" }))
    }
}

/// `[{ index, kind, status, instruction? }]` of every step.
pub(super) fn step_list(t: &Task) -> Vec<Value> {
    t.steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut v = json!({ "index": i, "kind": s.kind, "status": s.status });
            if let Some(instr) = &s.instruction {
                v["instruction"] = json!(instr);
            }
            v
        })
        .collect()
}

/// Help source of a sub-agent call.
pub(super) fn agent_source(binding: &SessionBinding) -> HelpSource {
    HelpSource::Agent { role: binding.role }
}
