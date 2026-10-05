//! The chats' orchestrators inside the runtime loop (`core-design.md` §17.3,
//! §17.8): one session per chat (`orchestrator:<chatId>`), started lazily when
//! there is something to send, in the chat's worktree (which never moves), and
//! started again after its process ended.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::driver::{Driver, Restarted};
use super::launch::FirstPrompts;
use super::sessions::{AgentPick, StoredSession};
use crate::acp::AgentError;
use crate::domain::{
    COMPACT_PROMPT, Chat, DomainCommand, INTERNAL_BRANCH_PREFIX, InboxItem, InboxKind,
    OrchestratorResume, ToolError, TurnOutcome, chat_of_session, check_target_branch,
    lost_session_note, orchestrator_session, render_batch,
};
use crate::git::{BranchWorktreeError, CreateBranchError};
use crate::mcp::{SessionBinding, ToolCall};
use crate::store::SessionRecord;

/// What a new chat is created in (`create_chat`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTarget {
    /// The worktree of this local branch (created by Yhtye if it is checked
    /// out nowhere).
    Branch(String),
    /// This existing worktree of the repository.
    Worktree(PathBuf),
}

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
    /// Chats whose orchestrator is running a `/compact` turn the user asked for.
    /// Its end is not the orchestrator's own (no group-finish reminder follows).
    compacting: HashSet<String>,
}

impl Orchestrators {
    /// Drops what is remembered about a deleted chat.
    fn forget(&mut self, chat: &str) {
        self.pending.remove(&orchestrator_session(chat));
        self.cut_off.remove(chat);
        self.failed.remove(chat);
        self.compacting.remove(chat);
    }
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
        self.chat_worktree(&chat)?;
        match self
            .execute(DomainCommand::UserMessage { chat, text })
            .await
        {
            Some(Err(e)) => Err(e),
            _ => Ok(()),
        }
    }

    /// The user's title for `chat` (`ChatTitled`).
    pub(super) async fn rename_chat(
        &mut self,
        chat: String,
        title: String,
    ) -> Result<(), ToolError> {
        match self
            .execute(DomainCommand::RenameChat { chat, title })
            .await
        {
            Some(Err(e)) => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes `chat`. Refused while its orchestrator is working (the user
    /// cancels the turn first; a delete never cuts one off) or a group of the
    /// chat is not finished; an idle orchestrator process is stopped once the
    /// chat is gone.
    pub(super) async fn delete_chat(&mut self, chat: String) -> Result<(), ToolError> {
        let key = orchestrator_session(&chat);
        let working = self.sessions.is_starting(&key)
            || (self.sessions.is_live(&key) && !self.sessions.is_idle(&key));
        if working && self.state.chat(&chat).is_some() {
            return Err(ToolError::invalid_state(format!(
                "the orchestrator of chat {chat} is working; stop its turn before deleting the chat"
            )));
        }
        if let Some(Err(e)) = self
            .execute(DomainCommand::DeleteChat { chat: chat.clone() })
            .await
        {
            return Err(e);
        }
        self.sessions.stop(&key);
        self.orchestrators.forget(&chat);
        Ok(())
    }

    /// Sends [`COMPACT_PROMPT`] as is (not as an inbox batch, so the harness
    /// sees a slash command) to the orchestrator of `chat`, which must be
    /// running and idle. Inbox entries arriving meanwhile wait for the turn to end.
    pub(super) fn compact_chat(&mut self, chat: &str) -> Result<(), ToolError> {
        if self.state.chat(chat).is_none() {
            return Err(ToolError::not_found(format!("no chat {chat}")));
        }
        let key = orchestrator_session(chat);
        if self.sessions.is_starting(&key) {
            return Err(ToolError::invalid_state(format!(
                "the orchestrator of chat {chat} is starting; compact its context once it is idle"
            )));
        }
        match self.sessions.prompt_now(&key, COMPACT_PROMPT) {
            Ok(()) => {
                self.orchestrators.compacting.insert(chat.to_string());
                Ok(())
            }
            Err(AgentError::Closed) => Err(ToolError::invalid_state(format!(
                "the orchestrator of chat {chat} is not running; there is no context to compact"
            ))),
            Err(AgentError::Busy) => Err(ToolError::invalid_state(format!(
                "the orchestrator of chat {chat} is working; compact its context once its turn ends"
            ))),
            Err(e) => Err(ToolError::internal(format!(
                "could not send {COMPACT_PROMPT} to the orchestrator of chat {chat}: {e}"
            ))),
        }
    }

    /// The worktree of `chat` if its directory still exists: `not_found` for
    /// an unknown chat, `invalid_state` for one whose worktree is gone.
    pub(super) fn chat_worktree(&self, chat: &str) -> Result<PathBuf, ToolError> {
        let c = self
            .state
            .chat(chat)
            .ok_or_else(|| ToolError::not_found(format!("no chat {chat}")))?;
        if c.worktree.is_dir() {
            return Ok(c.worktree.clone());
        }
        Err(ToolError::invalid_state(format!(
            "the working tree {} of chat {chat} no longer exists; its history can be read, \
             but nothing can be sent to it",
            c.worktree.display()
        )))
    }

    pub(super) async fn create_chat(&mut self, target: ChatTarget) -> Result<Chat, ToolError> {
        let worktree = match target {
            ChatTarget::Branch(branch) => self.branch_worktree(&branch).await?,
            ChatTarget::Worktree(dir) => self.existing_worktree(&dir).await?,
        };
        if let Some(Err(e)) = self.execute(DomainCommand::CreateChat { worktree }).await {
            return Err(e);
        }
        self.state
            .chats
            .last()
            .cloned()
            .ok_or_else(|| ToolError::internal("the chat was not created"))
    }

    /// The worktree of `branch`: where it is checked out, else a new Yhtye one.
    async fn branch_worktree(&self, branch: &str) -> Result<PathBuf, ToolError> {
        check_target_branch(branch)?;
        self.git
            .resolve_branch_worktree(branch)
            .await
            .map_err(|e| match e {
                BranchWorktreeError::BranchMissing(_) => {
                    ToolError::not_found(format!("no branch {branch}"))
                }
                BranchWorktreeError::Failed(m) => ToolError::internal(m),
            })
    }

    /// `dir` as git lists it, if it is an existing worktree that is not one of
    /// Yhtye's internal ones (on a `yhtye/*` branch).
    async fn existing_worktree(&self, dir: &Path) -> Result<PathBuf, ToolError> {
        let internal = |e: String| ToolError::internal(format!("could not read git: {e}"));
        let Some(listed) = self.git.find_worktree(dir).await.map_err(internal)? else {
            return Err(ToolError::not_found(format!(
                "{} is not a worktree of this repository",
                dir.display()
            )));
        };
        let branch = self.git.worktree_branch(&listed).await.map_err(internal)?;
        if branch.is_some_and(|b| b.starts_with(INTERNAL_BRANCH_PREFIX)) {
            return Err(ToolError::invalid_argument(format!(
                "{} is one of Yhtye's own worktrees; pick another one",
                listed.display()
            )));
        }
        Ok(listed)
    }

    /// Creates the branch in a Yhtye worktree, then its first chat there.
    pub(super) async fn create_branch(
        &mut self,
        name: String,
        from: Option<String>,
    ) -> Result<Chat, ToolError> {
        check_target_branch(&name)?;
        let dir = self.git.create_branch(&name, from.as_deref()).await?;
        match self.create_chat(ChatTarget::Worktree(dir)).await {
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
        let cwd = self.chat_worktree(chat).map_err(|e| e.message)?;
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
            // A new process: a compaction of an earlier one is over.
            self.orchestrators.compacting.remove(&chat);
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
        if self.orchestrators.compacting.remove(&chat) {
            // The user's `/compact` turn: nothing for the state machine (the
            // inbox is flushed as after every turn).
            return;
        }
        if outcome == TurnOutcome::Closed {
            self.orchestrators.cut_off.insert(chat.clone());
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

impl Driver {
    /// The branch the worktree of `chat` has checked out now: what a new group
    /// merges into (Stage 8e). `invalid_state` for a detached HEAD, a Yhtye
    /// branch or a worktree that is gone.
    pub(super) async fn checked_out_branch(&self, chat: &str) -> Result<String, ToolError> {
        let dir = self.chat_worktree(chat)?;
        match self.head_of(&dir).await? {
            Some(b) if !b.starts_with(INTERNAL_BRANCH_PREFIX) => Ok(b),
            head => Err(ToolError::invalid_state(format!(
                "the working tree {} is {}; a group merges into the branch checked out \
                 there, so check out a branch first",
                dir.display(),
                head.map_or("at a detached HEAD".to_string(), |b| format!("on {b}"))
            ))),
        }
    }

    /// `finish_group`'s `into` must be the branch the chat's worktree has
    /// checked out right now (`invalid_argument` naming what it has).
    pub(super) async fn check_into(&self, chat: &str, into: &str) -> Result<(), ToolError> {
        let dir = self.chat_worktree(chat)?;
        let head = self.head_of(&dir).await?;
        if head.as_deref() == Some(into) {
            return Ok(());
        }
        let now = head.map_or("a detached HEAD".to_string(), |b| format!("branch `{b}`"));
        Err(ToolError::invalid_argument(format!(
            "into must be the branch checked out in your working tree {} right now, which is \
             {now}, not `{into}`",
            dir.display()
        )))
    }

    async fn head_of(&self, dir: &Path) -> Result<Option<String>, ToolError> {
        self.git.worktree_branch(dir).await.map_err(|e| {
            ToolError::internal(format!(
                "could not read the branch of {}: {e}",
                dir.display()
            ))
        })
    }

    /// The stored session of `chat`'s orchestrator (what it was started with).
    pub(super) async fn stored_session(&mut self, chat: &str) -> Option<SessionRecord> {
        self.publisher.flush().await;
        let key = orchestrator_session(chat);
        match self.publisher.store().sessions(&self.cfg.project).await {
            Ok(all) => all.into_iter().find(|s| s.session_key == key),
            Err(e) => {
                tracing::error!("could not read the stored sessions: {e}");
                None
            }
        }
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
