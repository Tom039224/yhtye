//! Unit tests of the state machine. Every command goes through [`Sim::run`],
//! which also checks the event-sourcing invariant: replaying a transition's
//! events on the old state gives exactly the new state.

mod agents;
mod errors;
mod flows;
mod git;
mod table;

use std::collections::VecDeque;

use serde_json::{Value, json};

use super::*;
use crate::mcp::SessionBinding;
use crate::mcp::tools::{ToolName, parse_call};

type GitResponder = Box<dyn FnMut(&GitOp) -> GitResult>;

/// Test driver: a state plus a scripted git that answers git effects the way
/// the runtime does (inline, fed back as `GitDone`).
struct Sim {
    state: State,
    git: GitResponder,
}

/// Everything a command chain produced (git effects already answered).
#[derive(Debug, Default)]
struct Chain {
    events: Vec<DomainEvent>,
    effects: Vec<Effect>,
    reply: Option<Result<Value, ToolError>>,
}

fn noop_git(op: &GitOp) -> GitResult {
    match op {
        GitOp::PrepareWorkspace { task, .. } => GitResult::Workspace {
            path: format!("/wt/{task}").into(),
        },
        GitOp::FinishTask { .. } | GitOp::MergeGroup { .. } => GitResult::Merged {
            detail: "merged".into(),
        },
        GitOp::CreateGroupBranch { .. } | GitOp::RemoveWorkspace { .. } => GitResult::Done,
    }
}

fn orch() -> SessionBinding {
    SessionBinding::orchestrator("orchestrator", "P-1")
}

fn sub(task: &str, role: Role, step: usize) -> SessionBinding {
    SessionBinding {
        session: format!("{task}/{role}"),
        role,
        project: "P-1".into(),
        group: Some("G-1".into()),
        task: Some(task.into()),
        step: Some(step),
    }
}

fn agent(task: &str, role: Role, step: usize) -> AgentRef {
    AgentRef {
        task: task.into(),
        role,
        step,
    }
}

fn tool_cmd(binding: SessionBinding, tool: ToolName, args: Value) -> DomainCommand {
    let call = parse_call(tool, args.as_object().cloned()).expect("valid args");
    match call {
        crate::mcp::ToolCall::CreateGroup(args) => DomainCommand::CreateGroup {
            args,
            base_branch: Some("main".into()),
        },
        call => DomainCommand::Tool { binding, call },
    }
}

impl Sim {
    fn new() -> Self {
        Self::with_config(DomainConfig::default())
    }

    fn with_config(config: DomainConfig) -> Self {
        Self {
            state: State::new("P-1", config),
            git: Box::new(noop_git),
        }
    }

    /// One command, checking the replay invariant.
    fn step(&mut self, cmd: DomainCommand) -> Result<Transition, ToolError> {
        let (next, t) = decide(&self.state, cmd)?;
        let mut replay = self.state.clone();
        for e in &t.events {
            replay.apply(e);
        }
        assert_eq!(replay, next, "replaying the events must give the new state");
        self.state = next;
        Ok(t)
    }

    /// A command plus the git follow-ups, like the runtime's chain.
    fn run(&mut self, cmd: DomainCommand) -> Result<Chain, ToolError> {
        let mut chain = Chain::default();
        let mut queue = VecDeque::from([cmd]);
        let mut first = true;
        while let Some(cmd) = queue.pop_front() {
            let t = match self.step(cmd) {
                Ok(t) => t,
                Err(e) if first => return Err(e),
                Err(e) => panic!("follow-up failed: {e}"),
            };
            first = false;
            chain.events.extend(t.events);
            if t.reply.is_some() {
                chain.reply = t.reply;
            }
            for effect in t.effects {
                if let Effect::Git(op) = &effect {
                    let result = (self.git)(op);
                    queue.push_back(DomainCommand::GitDone {
                        op: op.clone(),
                        result,
                    });
                }
                chain.effects.push(effect);
            }
        }
        Ok(chain)
    }

    fn tool(&mut self, b: SessionBinding, tool: ToolName, args: Value) -> Result<Chain, ToolError> {
        self.run(tool_cmd(b, tool, args))
    }

    fn orch_ok(&mut self, tool: ToolName, args: Value) -> (Value, Chain) {
        let chain = self
            .tool(orch(), tool, args)
            .unwrap_or_else(|e| panic!("{tool:?} failed: {e}"));
        let reply = chain.reply.clone().expect("reply").expect("ok reply");
        (reply, chain)
    }

    fn orch_err(&mut self, tool: ToolName, args: Value) -> ErrorCode {
        let before = self.state.clone();
        let code = self.tool(orch(), tool, args).expect_err("must fail").code;
        assert_eq!(before, self.state, "a failed command changes nothing");
        code
    }

    fn sub_ok(&mut self, b: SessionBinding, tool: ToolName, args: Value) -> Chain {
        self.tool(b, tool, args)
            .unwrap_or_else(|e| panic!("{tool:?} failed: {e}"))
    }

    fn sub_err(&mut self, b: SessionBinding, tool: ToolName, args: Value) -> ErrorCode {
        self.tool(b, tool, args).expect_err("must fail").code
    }

    fn turn_ended(&mut self, a: AgentRef, outcome: TurnOutcome) -> Chain {
        self.run(DomainCommand::TurnEnded {
            agent: a,
            outcome,
            prompt_queued: false,
        })
        .expect("turn_ended never fails")
    }

    fn group(&mut self) {
        self.orch_ok(ToolName::CreateGroup, json!({"title": "g"}));
    }

    /// Creates a task in G-1 with `extra` merged into the default arguments.
    fn task(&mut self, extra: Value) -> Chain {
        let mut args = json!({"group_id": "G-1", "title": "t", "kind": "code",
                              "steps": [{"kind": "implement"}], "instruction": "do it"});
        if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
            a.extend(e.clone());
        }
        self.orch_ok(ToolName::CreateTask, args).1
    }

    /// Reports the running step of `task` done as `role` and ends the turn.
    fn report(&mut self, task: &str, role: Role, verdict: Option<&str>) -> Chain {
        let step = self.t(task).current;
        let mut args = json!({"result": format!("result of step {step}")});
        if let Some(v) = verdict {
            args["verdict"] = json!(v);
        }
        self.sub_ok(sub(task, role, step), ToolName::ReportStepDone, args);
        self.turn_ended(agent(task, role, step), TurnOutcome::EndTurn)
    }

    fn t(&self, id: &str) -> &Task {
        self.state.task(id).expect("task exists")
    }

    fn status(&self, id: &str) -> TaskStatus {
        self.t(id).status
    }

    fn inbox_kinds(&self) -> Vec<InboxKind> {
        self.state.inbox.iter().map(|e| e.item.kind).collect()
    }

    fn deliver_inbox(&mut self) {
        if let Some(up_to) = self.state.inbox.last().map(|e| e.id) {
            self.run(DomainCommand::InboxDelivered { up_to })
                .expect("deliver");
        }
    }
}

/// `(task, role, step)` of every `RunStep` effect.
fn run_steps(chain: &Chain) -> Vec<(String, Role, usize)> {
    chain
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::RunStep { agent, .. } => Some((agent.task.clone(), agent.role, agent.step)),
            _ => None,
        })
        .collect()
}

fn prompt_of(chain: &Chain) -> String {
    chain
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::RunStep { prompt, .. } => Some(prompt.clone()),
            Effect::PromptAgent { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("a prompt effect")
}

fn has_effect(chain: &Chain, f: impl Fn(&Effect) -> bool) -> bool {
    chain.effects.iter().any(f)
}
