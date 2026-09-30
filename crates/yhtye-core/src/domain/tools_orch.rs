//! Orchestrator tools that change state (`mcp-tools.md` §3).

use serde_json::{Value, json};

use super::chat_rules::check_target_branch;
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
        self.check_owner(binding, &call)?;
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
            ToolCall::GetStatus(a) => self.get_status(binding, &a),
            ToolCall::GetTask(a) => self.get_task(&a),
            ToolCall::ReportStepDone(a) => self.report_step_done(binding, a),
            ToolCall::Help(a) => self.help(binding, a),
        }?;
        self.reply(Ok(reply));
        Ok(())
    }

    /// `create_group` of the orchestrator of `chat`: the group merges into
    /// `base_branch` (what the chat's worktree has checked out now), and only
    /// one group per worktree may be open (Stage 8e).
    pub(super) fn create_group(
        &mut self,
        chat: &str,
        a: CreateGroupArgs,
        base_branch: String,
        taken: u32,
    ) -> Result<(), ToolError> {
        non_empty("title", &a.title)?;
        let worktree = self
            .state
            .chat(chat)
            .ok_or_else(|| ToolError::not_found(format!("no chat {chat}")))?
            .worktree
            .clone();
        if check_target_branch(&base_branch).is_err() {
            return Err(ToolError::invalid_state(format!(
                "the worktree {} has no branch checked out that a group can merge into \
                 (detached HEAD or a yhtye/* branch); check out a branch first",
                worktree.display()
            )));
        }
        if let Some(g) = self.state.open_group_in(&worktree) {
            let whose = if g.chat == chat {
                String::new()
            } else {
                format!(" (created by chat {})", g.chat)
            };
            return Err(ToolError::conflict(format!(
                "group {} in this worktree is still {}{whose}; finish_group or \
                 cancel_group it first",
                g.id,
                g.status.as_str()
            )));
        }
        let id = format!("G-{}", self.state.counters.groups.max(taken) + 1);
        let group_branch = super::state::group_branch(&id);
        self.emit(DomainEvent::GroupCreated {
            group: Group {
                id: id.clone(),
                chat: chat.to_string(),
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
                    effort: a.effort,
                }),
                review_agent: a.review_harness.map(|harness| AgentChoice {
                    harness,
                    model: a.review_model,
                    effort: a.review_effort,
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

    /// `finish_group`: an `active` group whose tasks are all terminal, or a
    /// `merge_blocked` one (the merge runs again, Stage 8e). With `into` (the
    /// runtime checked that the chat's worktree has it checked out) the group
    /// merges there instead of its recorded base branch.
    fn finish_group(&mut self, a: FinishGroupArgs) -> Reply {
        let g = self
            .state
            .group(&a.group_id)
            .ok_or_else(|| ToolError::not_found(format!("no group {}", a.group_id)))?;
        match g.status {
            GroupStatus::Active => self.check_tasks_finished(&a.group_id)?,
            GroupStatus::MergeBlocked => self.check_worktree_free(&a.group_id)?,
            other => {
                return Err(ToolError::invalid_state(format!(
                    "group {} is {}; only an active or merge_blocked group can be finished",
                    g.id,
                    other.as_str()
                )));
            }
        }
        let from = g.base_branch.clone();
        if let Some(into) = a.into.filter(|i| *i != from) {
            check_target_branch(&into)?;
            self.emit(DomainEvent::GroupBaseChanged {
                group: a.group_id.clone(),
                from,
                to: into,
            });
        }
        self.start_merge(&a.group_id, a.summary, MergeTrigger::FinishGroup)?;
        Ok(json!({ "group_id": a.group_id, "status": GroupStatus::Finishing }))
    }

    fn check_tasks_finished(&self, group: &str) -> Result<(), ToolError> {
        let open: Vec<String> = self
            .state
            .tasks_of(group)
            .filter(|t| !t.status.is_terminal())
            .map(|t| format!("{} ({})", t.id, t.status.as_str()))
            .collect();
        if open.is_empty() {
            return Ok(());
        }
        Err(ToolError::invalid_state(format!(
            "tasks not finished yet: {} (wait for them, or cancel_task them)",
            open.join(", ")
        )))
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

impl Tx {
    /// An orchestrator bound to a chat only reaches that chat's groups and
    /// tasks (`forbidden` otherwise). Unknown ids pass: the tool reports them.
    fn check_owner(&self, binding: &SessionBinding, call: &ToolCall) -> Result<(), ToolError> {
        let Some(chat) = binding.chat.as_deref() else {
            return Ok(());
        };
        let owner = match call {
            ToolCall::CreateTask(a) => self.state.chat_of_group(&a.group_id),
            ToolCall::FinishGroup(a) => self.state.chat_of_group(&a.group_id),
            ToolCall::CancelGroup(a) => self.state.chat_of_group(&a.group_id),
            ToolCall::GetStatus(a) => a
                .group_id
                .as_deref()
                .and_then(|g| self.state.chat_of_group(g)),
            ToolCall::SetInstruction(a) => self.state.chat_of_task(&a.task_id),
            ToolCall::ModifySteps(a) => self.state.chat_of_task(&a.task_id),
            ToolCall::ResolveCheckpoint(a) => self.state.chat_of_task(&a.task_id),
            ToolCall::CancelTask(a) => self.state.chat_of_task(&a.task_id),
            ToolCall::GetTask(a) => self.state.chat_of_task(&a.task_id),
            ToolCall::AnswerHelp(a) => self
                .state
                .help(&a.help_id)
                .and_then(|h| self.state.chat_of_task(&h.task)),
            _ => None,
        };
        match owner {
            Some(o) if o != chat => Err(ToolError::forbidden(format!(
                "that belongs to another chat ({o}); this chat is {chat}"
            ))),
            _ => Ok(()),
        }
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
