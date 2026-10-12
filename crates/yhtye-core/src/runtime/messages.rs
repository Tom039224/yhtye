//! Delivering the orchestrator's `message_task` messages (`core-design.md` §8).
//!
//! The state machine decides what may be sent and logs it; the ACP side happens
//! here, without blocking the loop. A message that has to wait for the agent keeps
//! the tool call's reply and is settled later by the steering answer, the end of
//! the agent's turn, a timeout, or a change of the task's state:
//!
//! - live session in a turn, steering supported: `_session/steering` injects the
//!   message into the turn;
//! - in a turn, no steering: `session/cancel`, then the message is the next
//!   prompt once the turn has ended — if the step is still unreported then (the
//!   turn end does not count as a silent one: no reminder, no `agent_stopped`);
//! - live session between turns: a normal prompt;
//! - starting or no session: handed back.
//!
//! A message is handed back (`returned`) whenever it cannot be delivered: the step
//! reported done first, the task left `running`, the turn did not end within
//! [`INTERRUPT_TIMEOUT`], the steering request failed (it may have reached the
//! agent, so the message is not sent again by another route).

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinSet;

use super::driver::{Driver, Reply};
use super::sessions::{Route, session_key};
use crate::acp::{AgentError, SteerOutcome};
use crate::domain::{
    AgentRef, DomainCommand, MessageOutcome, MessageVia, StepStatus, TaskStatus, ToolError,
    TurnOutcome, reported_detail, unavailable_hint,
};
use crate::mcp::SessionBinding;
use crate::mcp::tools::MessageTaskArgs;
use crate::prompts::orchestrator_message_prompt;

/// How long a message waits for the agent's turn to end (after `session/cancel`,
/// or when steering found the turn just over) before it is handed back.
pub(super) const INTERRUPT_TIMEOUT: Duration = Duration::from_secs(60);

const NOT_RUNNING: &str = "The agent's session is not running (it is still starting, or it \
    ended), so the message was not delivered. Send it again in a moment, or put it in the \
    step's instruction with modify_steps.";

/// Where a message waiting for its delivery stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// `_session/steering` is in flight.
    Steering,
    /// Waits for the end of the agent's turn: Yhtye cancelled it (`cancelled`), or
    /// steering found the turn over while its end was not processed yet.
    TurnEnd { cancelled: bool },
    /// Handed back after a timeout. The turn end, when it comes, must still not
    /// look like a silent one.
    Abandoned,
}

struct Pending {
    id: u64,
    agent: AgentRef,
    /// The orchestrator's text (not yet wrapped in the prompt).
    text: String,
    /// The `message_task` call's reply; `None` until the call has attached it
    /// and after the message was handed back.
    reply: Option<oneshot::Sender<Reply>>,
    stage: Stage,
}

/// What became of a message.
struct Resolution {
    outcome: MessageOutcome,
    via: Option<MessageVia>,
    detail: String,
}

impl Resolution {
    fn delivered(via: MessageVia, detail: impl Into<String>) -> Self {
        Self {
            outcome: MessageOutcome::Delivered,
            via: Some(via),
            detail: detail.into(),
        }
    }

    fn returned(detail: impl Into<String>) -> Self {
        Self {
            outcome: MessageOutcome::Returned,
            via: None,
            detail: detail.into(),
        }
    }
}

/// How a message gets to a session that is in a turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Steer it if the agent supports that, otherwise interrupt the turn.
    Auto,
    /// Cancel the turn and send the message when it has ended.
    Interrupt,
    /// Send the message when the turn has ended on its own.
    Wait,
}

/// Something a background task of [`Deliveries`] has to tell the loop.
pub(super) enum Wake {
    Steered {
        task: String,
        id: u64,
        result: Result<SteerOutcome, AgentError>,
    },
    TimedOut {
        task: String,
        id: u64,
    },
}

/// The messages waiting to be delivered, one per task at most.
#[derive(Default)]
pub(super) struct Deliveries {
    pending: HashMap<String, Pending>,
    next_id: u64,
    /// Steering requests in flight and timeouts.
    pub(super) wakes: JoinSet<Wake>,
}

impl Deliveries {
    /// Whether a message to `task` is still being delivered (a message that was
    /// handed back does not count).
    pub(super) fn blocks(&self, task: &str) -> bool {
        self.pending
            .get(task)
            .is_some_and(|p| p.stage != Stage::Abandoned)
    }

    fn start(&mut self, agent: AgentRef, text: String) -> Pending {
        self.next_id += 1;
        Pending {
            id: self.next_id,
            agent,
            text,
            reply: None,
            stage: Stage::Steering,
        }
    }

    /// Gives the message to `task` the `message_task` call's reply.
    fn attach(
        &mut self,
        task: &str,
        reply: oneshot::Sender<Reply>,
    ) -> Result<(), oneshot::Sender<Reply>> {
        match self.pending.get_mut(task) {
            Some(p) if p.reply.is_none() && p.stage != Stage::Abandoned => {
                p.reply = Some(reply);
                Ok(())
            }
            _ => Err(reply),
        }
    }

    /// The message `id` to `task`, if it is in `stage`.
    fn take(&mut self, task: &str, id: u64, stage: Stage) -> Option<Pending> {
        let p = self.pending.get(task)?;
        (p.id == id && p.stage == stage)
            .then(|| self.pending.remove(task))
            .flatten()
    }

    /// The message to `task` that waits for the end of a turn of session `key`.
    fn take_for_turn_end(&mut self, task: &str, key: &str) -> Option<Pending> {
        let p = self.pending.get(task)?;
        let waits = matches!(p.stage, Stage::TurnEnd { .. } | Stage::Abandoned);
        (waits && session_key(&p.agent) == key)
            .then(|| self.pending.remove(task))
            .flatten()
    }

    fn insert(&mut self, p: Pending) {
        self.pending.insert(p.agent.task.clone(), p);
    }
}

/// What the end of an agent's turn means for a message waiting for it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TurnEnd {
    /// Send the message as the next prompt. `interrupted`: the turn was cancelled.
    Send { interrupted: bool },
    /// Hand the message back; the turn end goes to the state machine as it is.
    Return(String),
    /// The message was handed back earlier: a cancelled turn that left the step
    /// unfinished counts as a plain end (the state machine then reminds the agent).
    ResumeStep,
    /// Nothing to do.
    Ignore,
}

/// Decides what the end of a turn (`outcome`) means for a message in `stage`,
/// given whether the step is still unreported.
pub(super) fn turn_end(
    stage: Stage,
    agent: &AgentRef,
    step_running: bool,
    outcome: &TurnOutcome,
) -> TurnEnd {
    match (stage, step_running, outcome) {
        (Stage::Abandoned, true, TurnOutcome::Cancelled) => TurnEnd::ResumeStep,
        (Stage::Abandoned | Stage::Steering, ..) => TurnEnd::Ignore,
        (Stage::TurnEnd { .. }, false, _) => {
            TurnEnd::Return(reported_detail(agent.role, agent.step))
        }
        (Stage::TurnEnd { .. }, true, TurnOutcome::EndTurn | TurnOutcome::Cancelled) => {
            TurnEnd::Send {
                interrupted: matches!(outcome, TurnOutcome::Cancelled),
            }
        }
        (Stage::TurnEnd { .. }, true, other) => TurnEnd::Return(format!(
            "The agent's turn ended abnormally ({}) before the message could be delivered; \
             Yhtye reports that to you separately.",
            describe(other)
        )),
    }
}

fn describe(outcome: &TurnOutcome) -> String {
    match outcome {
        TurnOutcome::EndTurn => "end_turn".into(),
        TurnOutcome::Cancelled => "cancelled".into(),
        TurnOutcome::Stopped(reason) => reason.clone(),
        TurnOutcome::Closed => "its process went away".into(),
        TurnOutcome::Error(e) => e.clone(),
    }
}

impl Driver {
    /// `message_task`: answered once the message is delivered or handed back.
    /// The loop is not blocked meanwhile: a waiting message keeps `reply`.
    pub(super) async fn message_task(
        &mut self,
        binding: SessionBinding,
        args: MessageTaskArgs,
        reply: oneshot::Sender<Reply>,
    ) {
        let task = args.task_id.clone();
        let pending = self.deliveries.blocks(&task);
        let cmd = DomainCommand::MessageTask {
            binding,
            args,
            pending,
        };
        match self.execute(cmd).await {
            // Refused, or settled at once.
            Some(answer) => {
                let _ = reply.send(answer);
            }
            // Waiting for the agent: the delivery started in `deliver_message`.
            None => {
                if let Err(reply) = self.deliveries.attach(&task, reply) {
                    tracing::error!("message to {task}: no delivery was started");
                    let _ = reply.send(Err(ToolError::internal(
                        "Yhtye neither delivered the message nor kept it",
                    )));
                }
            }
        }
    }

    /// Starts delivering the orchestrator's message to `agent` (the effect of an
    /// accepted `message_task`). Returns the settling command when it is settled
    /// right away.
    pub(super) fn deliver_message(
        &mut self,
        agent: AgentRef,
        text: String,
    ) -> Option<DomainCommand> {
        let p = self.deliveries.start(agent, text);
        let (p, resolution) = self.route_message(p, Mode::Auto)?;
        Some(resolved_cmd(&p.agent, resolution))
    }

    /// Sends the message by the route `mode` allows. `None`: it waits (for the
    /// steering answer or the turn end) and is kept; otherwise it is settled.
    fn route_message(&mut self, mut p: Pending, mode: Mode) -> Option<(Pending, Resolution)> {
        let key = session_key(&p.agent);
        match self.sessions.route(&key) {
            Route::Absent | Route::Starting => Some((p, Resolution::returned(NOT_RUNNING))),
            Route::Idle => {
                let body = orchestrator_message_prompt(&p.text, false);
                self.sessions.queue_and_deliver(&key, body);
                let detail = "The agent was between turns; the message was sent to it as a new \
                              prompt.";
                Some((p, Resolution::delivered(MessageVia::Prompt, detail)))
            }
            Route::InTurn { steering } => {
                if mode == Mode::Auto && steering {
                    let body = orchestrator_message_prompt(&p.text, false);
                    let Some(answer) = self.sessions.steer_text(&key, &body) else {
                        return Some((p, Resolution::returned(NOT_RUNNING)));
                    };
                    let (task, id) = (p.agent.task.clone(), p.id);
                    self.deliveries.wakes.spawn(async move {
                        let result = answer.await;
                        Wake::Steered { task, id, result }
                    });
                    p.stage = Stage::Steering;
                    self.deliveries.insert(p);
                    return None;
                }
                let cancel = mode != Mode::Wait;
                if cancel {
                    self.sessions.cancel_turn(&key);
                }
                p.stage = Stage::TurnEnd { cancelled: cancel };
                let (task, id) = (p.agent.task.clone(), p.id);
                self.deliveries.wakes.spawn(async move {
                    tokio::time::sleep(INTERRUPT_TIMEOUT).await;
                    Wake::TimedOut { task, id }
                });
                self.deliveries.insert(p);
                None
            }
        }
    }

    /// A steering answer or a timeout arrived.
    pub(super) async fn on_message_wake(&mut self, wake: Wake) {
        match wake {
            Wake::Steered { task, id, result } => {
                if let Some(p) = self.deliveries.take(&task, id, Stage::Steering) {
                    self.steered(p, result).await;
                }
            }
            Wake::TimedOut { task, id } => self.timed_out(&task, id).await,
        }
    }

    async fn steered(&mut self, p: Pending, result: Result<SteerOutcome, AgentError>) {
        let key = session_key(&p.agent);
        let resolution = match result {
            Ok(SteerOutcome::Injected) => {
                self.sessions
                    .announce_prompt(&key, &orchestrator_message_prompt(&p.text, false));
                Resolution::delivered(
                    MessageVia::Steering,
                    "The message was injected into the agent's running turn; it takes it into \
                     account and continues the step.",
                )
            }
            Ok(SteerOutcome::StartedNewTurn) => {
                tracing::warn!("steering {key}: the agent started a turn of its own");
                self.sessions
                    .announce_prompt(&key, &orchestrator_message_prompt(&p.text, false));
                Resolution::delivered(
                    MessageVia::Steering,
                    "Warning: the agent had no running turn by then and started one of its own \
                     for the message, which Yhtye does not follow. If the step does not \
                     finish, check the task with get_task.",
                )
            }
            // The turn ended just before the message arrived.
            Ok(SteerOutcome::PromptRequired) => return self.missed_steer(p).await,
            Err(AgentError::SteeringUnsupported) => {
                return self.reroute(p, Mode::Interrupt).await;
            }
            Err(AgentError::Closed) => Resolution::returned(
                "The agent's session ended while the message was being delivered.",
            ),
            // It may have reached the agent: not sent again by another route.
            Err(e) => Resolution::returned(format!(
                "Steering the message into the agent's turn failed ({e}); it may or may not \
                 have reached the agent, so it was not sent again. Look at the task's \
                 conversation before sending it again."
            )),
        };
        self.settle_pending(p, resolution).await;
    }

    /// Steering found no running turn: send the message when the turn end has
    /// been processed, if the step is still unreported then.
    async fn missed_steer(&mut self, p: Pending) {
        if self.step_running(&p.agent) {
            return self.reroute(p, Mode::Wait).await;
        }
        let detail = reported_detail(p.agent.role, p.agent.step);
        self.settle_pending(p, Resolution::returned(detail)).await;
    }

    async fn reroute(&mut self, p: Pending, mode: Mode) {
        if let Some((p, resolution)) = self.route_message(p, mode) {
            self.settle_pending(p, resolution).await;
        }
    }

    async fn timed_out(&mut self, task: &str, id: u64) {
        let Some(p) = self.deliveries.pending.get_mut(task) else {
            return;
        };
        if p.id != id || !matches!(p.stage, Stage::TurnEnd { .. }) {
            return;
        }
        let (agent, reply) = (p.agent.clone(), p.reply.take());
        p.stage = Stage::Abandoned;
        let detail = format!(
            "The agent's turn did not end within {} s, so the message was not delivered. Look \
             at the task with get_task; cancel_task, or send the message again later.",
            INTERRUPT_TIMEOUT.as_secs()
        );
        self.settle(&agent, reply, Resolution::returned(detail))
            .await;
    }

    /// The end of a turn of the session of `agent`: delivers the message waiting
    /// for it, or hands it back. Returns the outcome as the state machine is to
    /// see it. A delivered message is queued as the session's next prompt here,
    /// so the state machine counts the turn as one with a prompt queued.
    pub(super) async fn message_at_turn_end(
        &mut self,
        agent: &AgentRef,
        outcome: TurnOutcome,
    ) -> TurnOutcome {
        let key = session_key(agent);
        let Some(p) = self.deliveries.take_for_turn_end(&agent.task, &key) else {
            return outcome;
        };
        let running = self.step_running(&p.agent);
        match turn_end(p.stage, &p.agent, running, &outcome) {
            TurnEnd::Send { interrupted } => {
                let body = orchestrator_message_prompt(&p.text, interrupted);
                if self.sessions.queue_text(&key, body) {
                    let cancelled = matches!(p.stage, Stage::TurnEnd { cancelled: true });
                    let resolution = turn_end_delivery(cancelled);
                    self.settle_pending(p, resolution).await;
                } else {
                    self.settle_pending(p, Resolution::returned(NOT_RUNNING))
                        .await;
                }
                outcome
            }
            TurnEnd::Return(detail) => {
                self.settle_pending(p, Resolution::returned(detail)).await;
                outcome
            }
            TurnEnd::ResumeStep => TurnOutcome::EndTurn,
            TurnEnd::Ignore => outcome,
        }
    }

    /// Hands back the messages that can no longer be delivered because the task
    /// moved on. Runs after every loop turn.
    pub(super) async fn settle_messages(&mut self) {
        let stuck: Vec<(String, String)> = self
            .deliveries
            .pending
            .values()
            .filter_map(|p| {
                self.message_obstacle(p)
                    .map(|why| (p.agent.task.clone(), why))
            })
            .collect();
        for (task, why) in stuck {
            let Some(p) = self.deliveries.pending.remove(&task) else {
                continue;
            };
            if p.stage != Stage::Abandoned {
                self.settle_pending(p, Resolution::returned(why)).await;
            }
        }
    }

    /// Why `p` can no longer be delivered, if so.
    fn message_obstacle(&self, p: &Pending) -> Option<String> {
        let Some(t) = self.state.task(&p.agent.task) else {
            return Some("The task no longer exists.".into());
        };
        if t.status != TaskStatus::Running {
            return Some(format!(
                "Task {} is now {}, so the message was not delivered; {}.",
                t.id,
                t.status.as_str(),
                unavailable_hint(t.status)
            ));
        }
        let step = t
            .current_step()
            .filter(|s| t.current == p.agent.step && s.kind.role() == Some(p.agent.role));
        match step {
            None => Some(format!(
                "Step {} of task {} has finished and the task moved on, so the message was not \
                 delivered. For more work, add a step with modify_steps or create a new task.",
                p.agent.step + 1,
                t.id
            )),
            // A steering request in flight is answered by the agent.
            Some(s) if s.status == StepStatus::Done && p.stage != Stage::Steering => {
                Some(reported_detail(p.agent.role, p.agent.step))
            }
            Some(_) => None,
        }
    }

    /// Hands back every message still waiting (Yhtye is stopping).
    pub(super) async fn return_messages(&mut self, why: &str) {
        let tasks: Vec<String> = self.deliveries.pending.keys().cloned().collect();
        for task in tasks {
            if let Some(p) = self.deliveries.pending.remove(&task)
                && p.stage != Stage::Abandoned
            {
                self.settle_pending(p, Resolution::returned(why)).await;
            }
        }
    }

    /// Whether the step `agent` works on is the task's running step.
    fn step_running(&self, agent: &AgentRef) -> bool {
        self.state.task(&agent.task).is_some_and(|t| {
            t.status == TaskStatus::Running
                && t.current == agent.step
                && t.current_step().is_some_and(|s| {
                    s.status == StepStatus::Running && s.kind.role() == Some(agent.role)
                })
        })
    }

    async fn settle_pending(&mut self, p: Pending, resolution: Resolution) {
        self.settle(&p.agent, p.reply, resolution).await;
    }

    /// Logs the result in the state machine and answers the `message_task` call.
    async fn settle(
        &mut self,
        agent: &AgentRef,
        reply: Option<oneshot::Sender<Reply>>,
        resolution: Resolution,
    ) {
        let answer = self
            .execute(resolved_cmd(agent, resolution))
            .await
            .unwrap_or_else(|| Err(ToolError::internal("the message produced no answer")));
        if let Some(tx) = reply {
            let _ = tx.send(answer);
        }
    }
}

fn resolved_cmd(agent: &AgentRef, r: Resolution) -> DomainCommand {
    DomainCommand::MessageResolved {
        task: agent.task.clone(),
        step: agent.step,
        outcome: r.outcome,
        via: r.via,
        detail: r.detail,
    }
}

/// A message sent as the prompt after a turn end.
fn turn_end_delivery(cancelled: bool) -> Resolution {
    if cancelled {
        Resolution::delivered(
            MessageVia::Interrupt,
            "The agent's running turn was cancelled and the message sent to it as the next \
             prompt (its work so far is kept).",
        )
    } else {
        Resolution::delivered(
            MessageVia::Prompt,
            "The agent's turn had just ended; the message was sent to it as the next prompt.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Role;

    fn agent() -> AgentRef {
        AgentRef {
            task: "T-1".into(),
            role: Role::Implementer,
            step: 0,
        }
    }

    fn waiting(cancelled: bool) -> Stage {
        Stage::TurnEnd { cancelled }
    }

    #[test]
    fn an_unreported_step_gets_the_message_after_a_cancelled_or_plain_turn_end() {
        for (outcome, interrupted) in [
            (TurnOutcome::Cancelled, true),
            (TurnOutcome::EndTurn, false),
        ] {
            assert_eq!(
                turn_end(waiting(true), &agent(), true, &outcome),
                TurnEnd::Send { interrupted }
            );
        }
    }

    #[test]
    fn a_reported_step_hands_the_message_back_whatever_the_turn_ended_with() {
        for outcome in [
            TurnOutcome::Cancelled,
            TurnOutcome::EndTurn,
            TurnOutcome::Closed,
        ] {
            let TurnEnd::Return(detail) = turn_end(waiting(true), &agent(), false, &outcome) else {
                panic!("not handed back for {outcome:?}");
            };
            assert!(detail.contains("already reported step 1 done"), "{detail}");
        }
    }

    #[test]
    fn an_abnormal_turn_end_hands_the_message_back() {
        for outcome in [
            TurnOutcome::Stopped("max_tokens".into()),
            TurnOutcome::Error("boom".into()),
            TurnOutcome::Closed,
        ] {
            assert!(matches!(
                turn_end(waiting(false), &agent(), true, &outcome),
                TurnEnd::Return(_)
            ));
        }
    }

    #[test]
    fn a_message_handed_back_after_a_timeout_still_keeps_the_step_going() {
        assert_eq!(
            turn_end(Stage::Abandoned, &agent(), true, &TurnOutcome::Cancelled),
            TurnEnd::ResumeStep
        );
        for (running, outcome) in [
            (true, TurnOutcome::EndTurn),
            (false, TurnOutcome::Cancelled),
        ] {
            assert_eq!(
                turn_end(Stage::Abandoned, &agent(), running, &outcome),
                TurnEnd::Ignore
            );
        }
    }

    #[test]
    fn a_steering_request_in_flight_is_not_settled_by_a_turn_end() {
        assert_eq!(
            turn_end(Stage::Steering, &agent(), true, &TurnOutcome::EndTurn),
            TurnEnd::Ignore
        );
    }

    #[test]
    fn one_message_per_task_and_a_handed_back_one_does_not_block() {
        let mut d = Deliveries::default();
        assert!(!d.blocks("T-1"));
        let mut p = d.start(agent(), "hi".into());
        p.stage = waiting(true);
        let id = p.id;
        d.insert(p);
        assert!(d.blocks("T-1"));
        assert!(d.take("T-1", id, Stage::Steering).is_none(), "wrong stage");
        assert!(d.take("T-1", id + 1, waiting(true)).is_none(), "wrong id");

        let (tx, _rx) = oneshot::channel();
        assert!(d.attach("T-1", tx).is_ok());
        let (tx, _rx) = oneshot::channel();
        assert!(d.attach("T-1", tx).is_err(), "already has a reply");

        d.pending.get_mut("T-1").expect("pending").stage = Stage::Abandoned;
        assert!(!d.blocks("T-1"));
        let key = session_key(&agent());
        assert!(d.take_for_turn_end("T-1", &key).is_some());
        assert!(d.take_for_turn_end("T-1", &key).is_none());
    }

    #[test]
    fn a_turn_end_of_another_session_does_not_take_the_message() {
        let mut d = Deliveries::default();
        let mut p = d.start(agent(), "hi".into());
        p.stage = waiting(false);
        d.insert(p);
        assert!(d.take_for_turn_end("T-1", "T-1/review-1").is_none());
        assert!(d.take_for_turn_end("T-1", "T-1/implementer").is_some());
    }
}
