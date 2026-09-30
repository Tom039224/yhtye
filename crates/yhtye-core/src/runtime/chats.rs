//! The chats' orchestrators inside the runtime loop (`core-design.md` §17.3):
//! one session per chat (`orchestrator:<chatId>`), started lazily when there is
//! something to send, in the working tree of the chat's branch, and started
//! again after its process ended.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use super::driver::{Driver, Restarted};
use super::launch::FirstPrompts;
use super::sessions::{AgentPick, StoredSession};
use crate::domain::{
    Chat, DomainCommand, InboxItem, InboxKind, OrchestratorResume, ToolError, TurnOutcome,
    chat_of_session, check_target_branch, lost_session_note, orchestrator_session, render_batch,
};
use crate::git::CreateBranchError;
use crate::mcp::{SessionBinding, ToolCall};
use crate::store::SessionRecord;

/// Bookkeeping of the orchestrators being started.
#[derive(Default)]
pub(super) struct Orchestrators {
    /// Inbox entries sent along with a start in progress (session key → chat and
    /// last entry): they are marked delivered once the session is up, so a start
    /// that fails leaves them in the inbox.
    pending: HashMap<String, (String, u64)>,
    /// Chats whose turn was cut off by their process ending.
    cut_off: HashSet<String>,
    /// The newest inbox entry of a chat when its start failed. Nothing is retried
    /// until a newer entry (or the next app start) gives a new reason.
    failed: HashMap<String, u64>,
    /// Chats whose running orchestrator is in a directory its branch has left
    /// but was in a turn when that was seen: looked at again when the turn ends.
    pub(super) stale_cwd: HashSet<String>,
}

/// Where and how an orchestrator is started.
pub(super) struct StartPlan {
    cwd: PathBuf,
    /// The stored session to restore (`None`: start a new one).
    resume: Option<StoredSession>,
    pub(super) resume_info: OrchestratorResume,
}

impl Driver {
    /// The user's message to `chat`: checked, then queued in its inbox (the
    /// orchestrator is started by the next inbox flush).
    pub(super) async fn user_message(
        &mut self,
        chat: String,
        text: String,
    ) -> Result<(), ToolError> {
        self.follow_branch(&chat).await?;
        match self
            .execute(DomainCommand::UserMessage { chat, text })
            .await
        {
            Some(Err(e)) => Err(e),
            _ => Ok(()),
        }
    }

    pub(super) async fn create_chat(&mut self, branch: String) -> Result<Chat, ToolError> {
        check_target_branch(&branch)?;
        match self.git.branch_exists(&branch).await {
            Ok(true) => {}
            Ok(false) => return Err(ToolError::not_found(format!("no branch {branch}"))),
            Err(e) => {
                return Err(ToolError::internal(format!(
                    "could not check branch {branch}: {e}"
                )));
            }
        }
        if let Some(Err(e)) = self.execute(DomainCommand::CreateChat { branch }).await {
            return Err(e);
        }
        self.state
            .chats
            .last()
            .cloned()
            .ok_or_else(|| ToolError::internal("the chat was not created"))
    }

    /// Creates the branch in a Yhtye worktree, then its first chat.
    pub(super) async fn create_branch(
        &mut self,
        name: String,
        from: Option<String>,
    ) -> Result<Chat, ToolError> {
        check_target_branch(&name)?;
        self.git.create_branch(&name, from.as_deref()).await?;
        match self.create_chat(name.clone()).await {
            Ok(chat) => Ok(chat),
            Err(e) => {
                // No chat: undo the branch, or a retry would find it existing.
                if let Err(undo) = self.git.discard_branch(&name).await {
                    tracing::warn!("could not remove branch {name} again: {undo}");
                }
                Err(e)
            }
        }
    }

    /// The chat a user action on a task or group concerns.
    pub(super) fn chat_of_call(&self, call: &ToolCall) -> Option<String> {
        let chat = match call {
            ToolCall::CancelTask(a) => self.state.chat_of_task(&a.task_id),
            ToolCall::CancelGroup(a) => self.state.chat_of_group(&a.group_id),
            _ => None,
        };
        chat.map(str::to_string)
    }

    /// Plans the start of every chat to restore; a chat whose worktree cannot be
    /// found is reported and left alone.
    pub(super) async fn plan_restore(&mut self, r: &Restarted) -> Vec<(String, StartPlan)> {
        let mut plans = Vec::new();
        for chat in &r.chats {
            let key = orchestrator_session(chat);
            let record = r.records.iter().find(|s| s.session_key == key);
            match self.plan_start(chat, record).await {
                Ok(plan) => plans.push((chat.clone(), plan)),
                Err(e) => self.start_failed(chat, &e),
            }
        }
        plans
    }

    async fn plan_start(
        &mut self,
        chat: &str,
        record: Option<&SessionRecord>,
    ) -> Result<StartPlan, String> {
        let branch = self
            .follow_branch(chat)
            .await
            .map_err(|e| e.message.clone())?;
        let cwd = self
            .git
            .resolve_branch_worktree(&branch)
            .await
            .map_err(|e| e.to_string())?;
        // OpenCode wants the same directory for `session/load`; a session that
        // would start elsewhere is not restored.
        let same_dir = record.and_then(|r| r.cwd.as_deref()) == Some(&*cwd.to_string_lossy());
        let resume = record.filter(|_| same_dir).map(|r| StoredSession {
            acp_session_id: r.acp_session_id.clone(),
            agent: r.agent.clone(),
        });
        let resume_info = OrchestratorResume {
            had_session: record.is_some(),
            restored: resume.is_some(),
            turn_was_running: record.is_some_and(|r| r.turn_running)
                | self.orchestrators.cut_off.remove(chat),
        };
        Ok(StartPlan {
            cwd,
            resume,
            resume_info,
        })
    }

    /// Starts the orchestrator of `chat` in the background with what is in its
    /// inbox as the first prompt (a state summary in front of it if the stored
    /// session cannot be restored).
    pub(super) fn launch_orchestrator(&mut self, chat: &str, plan: StartPlan) {
        let key = orchestrator_session(chat);
        let items: Vec<InboxItem> = self.state.inbox_of(chat).map(|e| e.item.clone()).collect();
        let up_to = self.state.inbox_of(chat).last().map(|e| e.id);
        let fallback = plan.resume.is_some().then(|| {
            let lost = InboxItem::new(
                InboxKind::Restarted,
                &[],
                lost_session_note(&self.state, chat),
            );
            render_batch(&[vec![lost], items.clone()].concat())
        });
        let queued = if items.is_empty() {
            VecDeque::new()
        } else {
            VecDeque::from([render_batch(&items)])
        };
        let binding = SessionBinding::orchestrator(key.clone(), self.cfg.project.clone(), chat);
        let first = FirstPrompts { queued, fallback };
        self.sessions.launch(
            binding,
            plan.cwd,
            first,
            plan.resume,
            AgentPick::orchestrator(),
        );
        if !self.sessions.is_starting(&key) {
            // `launch` already reported why (the session failed to start).
            self.orchestrators
                .failed
                .insert(chat.to_string(), up_to.unwrap_or(0));
        } else if let Some(up_to) = up_to {
            self.orchestrators
                .pending
                .insert(key, (chat.to_string(), up_to));
        }
    }

    fn start_failed(&mut self, chat: &str, error: &str) {
        let last = self.state.inbox_of(chat).last().map_or(0, |e| e.id);
        self.orchestrators.failed.insert(chat.to_string(), last);
        self.sessions.failed_text(
            &orchestrator_session(chat),
            format!("could not start the orchestrator of chat {chat}: {error}"),
        );
    }

    /// A background start of `key` finished: the inbox entries sent with it are
    /// delivered if it is up; if it failed for good they stay in the inbox.
    pub(super) async fn orchestrator_spawned(&mut self, key: &str) {
        let Some(chat) = chat_of_session(key).map(str::to_string) else {
            return;
        };
        if self.sessions.is_live(key) {
            if let Some((chat, up_to)) = self.orchestrators.pending.remove(key) {
                self.execute(DomainCommand::InboxDelivered { chat, up_to })
                    .await;
            }
        } else if !self.sessions.is_starting(key) {
            self.orchestrators.pending.remove(key);
            let last = self.state.inbox_of(&chat).last().map_or(0, |e| e.id);
            self.orchestrators.failed.insert(chat, last);
        }
    }

    pub(super) async fn orchestrator_turn_ended(
        &mut self,
        chat: String,
        outcome: TurnOutcome,
        prompt_queued: bool,
    ) {
        if outcome == TurnOutcome::Closed {
            self.orchestrators.cut_off.insert(chat.clone());
        }
        // The orchestrator may have renamed its branch in this turn; the domain
        // (a group Yhtye finishes for it) must see the new name.
        if let Err(e) = self.follow_branch(&chat).await {
            tracing::debug!("{e}");
        }
        if self.orchestrators.stale_cwd.contains(&chat) {
            self.realign_orchestrator(&chat).await;
        }
        self.execute(DomainCommand::OrchestratorTurnEnded {
            chat,
            outcome,
            prompt_queued,
        })
        .await;
    }

    /// Sends every chat's undelivered inbox entries to its orchestrator: now if
    /// it is idle, after starting it (again) if it is not running.
    pub(super) async fn flush_inbox(&mut self) {
        let mut chats: Vec<String> = Vec::new();
        for e in &self.state.inbox {
            if !chats.contains(&e.chat) {
                chats.push(e.chat.clone());
            }
        }
        for chat in chats {
            self.flush_chat(&chat).await;
        }
    }

    async fn flush_chat(&mut self, chat: &str) {
        let key = orchestrator_session(chat);
        let Some(up_to) = self.state.inbox_of(chat).last().map(|e| e.id) else {
            return;
        };
        if self.sessions.is_idle(&key) {
            // Before delivering: has the branch been renamed or moved?
            if let Err(e) = self.follow_branch(chat).await {
                tracing::debug!("{e}");
            }
            self.realign_orchestrator(chat).await;
        }
        if self.sessions.is_idle(&key) {
            let items: Vec<_> = self.state.inbox_of(chat).map(|e| e.item.clone()).collect();
            self.sessions.queue_and_deliver(&key, render_batch(&items));
            let chat = chat.to_string();
            self.execute(DomainCommand::InboxDelivered { chat, up_to })
                .await;
        } else if self.sessions.is_live(&key) || self.sessions.is_starting(&key) {
            // Busy: it is flushed when its turn ends or its start finishes.
        } else if self
            .orchestrators
            .failed
            .get(chat)
            .is_none_or(|f| *f < up_to)
        {
            self.start_lazily(chat).await;
        }
    }

    /// Starts an orchestrator that is not running (never started, ended, or
    /// stopped by an earlier shutdown): restores its stored session if the
    /// working tree is the same, and tells it what it missed.
    async fn start_lazily(&mut self, chat: &str) {
        let record = self.stored_session(chat).await;
        let plan = match self.plan_start(chat, record.as_ref()).await {
            Ok(plan) => plan,
            Err(e) => return self.start_failed(chat, &e),
        };
        let r = plan.resume_info;
        if (r.had_session && !r.restored) || r.turn_was_running {
            let chat = chat.to_string();
            self.execute(DomainCommand::TellOrchestrator { chat, resume: r })
                .await;
        }
        self.launch_orchestrator(chat, plan);
    }
}

impl From<CreateBranchError> for ToolError {
    fn from(e: CreateBranchError) -> Self {
        match e {
            CreateBranchError::InvalidName(m) => Self::invalid_argument(m),
            CreateBranchError::Exists(_) => Self::conflict(e.to_string()),
            CreateBranchError::FromMissing(_) => Self::not_found(e.to_string()),
            CreateBranchError::Failed(m) => Self::internal(m),
        }
    }
}
