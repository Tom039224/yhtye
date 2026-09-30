//! A chat keeps up with its branch (`orchestration-model.md` §6.1, Stage 8d):
//! a branch that was renamed is followed, and an orchestrator whose branch is no
//! longer checked out in its working directory is started again where it is.

use std::path::Path;

use super::driver::Driver;
use crate::domain::{DomainCommand, INTERNAL_BRANCH_PREFIX, ToolError, orchestrator_session};
use crate::store::SessionRecord;

impl Driver {
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

    /// The branch `chat` works on, after following a rename: if it does not
    /// exist any more but the chat's recorded working directory now has another
    /// branch checked out, the chat moves there (`ChatBranchChanged`). `invalid_state`
    /// if it is gone without a trace of a rename, `not_found` for an unknown chat.
    pub(super) async fn follow_branch(&mut self, chat: &str) -> Result<String, ToolError> {
        let branch = self
            .state
            .chat(chat)
            .ok_or_else(|| ToolError::not_found(format!("no chat {chat}")))?
            .branch
            .clone();
        match self.git.branch_exists(&branch).await {
            Ok(true) => return Ok(branch),
            Ok(false) => {}
            Err(e) => {
                return Err(ToolError::internal(format!(
                    "could not check branch {branch}: {e}"
                )));
            }
        }
        if let Some(to) = self.renamed_to(chat, &branch).await {
            let cmd = DomainCommand::ChatBranchChanged {
                chat: chat.to_string(),
                to: to.clone(),
            };
            if let Some(Err(e)) = self.execute(cmd).await {
                tracing::warn!("could not follow {branch} to {to}: {e}");
            } else {
                tracing::info!("chat {chat} follows its branch {branch} renamed to {to}");
                return Ok(to);
            }
        }
        Err(ToolError::invalid_state(format!(
            "branch {branch} was deleted or renamed, so chat {chat} can no longer work on it"
        )))
    }

    /// Where the missing branch `old` of `chat` went: the branch its recorded
    /// working directory has checked out now (`git branch -m` moves HEAD of the
    /// worktree that has the branch), if that is a different, existing one whose
    /// reflog records the rename from `old`.
    async fn renamed_to(&mut self, chat: &str, old: &str) -> Option<String> {
        let cwd = self.stored_session(chat).await?.cwd?;
        let to = match self.git.worktree_branch(Path::new(&cwd)).await {
            Ok(Some(to)) => to,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!("could not read the branch checked out in {cwd}: {e}");
                return None;
            }
        };
        if to == old || to.starts_with(INTERNAL_BRANCH_PREFIX) {
            return None;
        }
        if !matches!(self.git.branch_exists(&to).await, Ok(true)) {
            return None;
        }
        // Checked out is not enough (the user may have switched away and deleted
        // the old branch): the reflog of `to` must say it was renamed from `old`.
        match self.git.was_renamed(old, &to).await {
            Ok(true) => Some(to),
            Ok(false) => None,
            Err(e) => {
                tracing::warn!("could not read the reflog of {to}: {e}");
                None
            }
        }
    }

    /// Makes sure the running orchestrator of `chat` works where its branch is
    /// checked out now: the branch's working tree (resolved again, which may make
    /// a Yhtye one) is compared with the directory the session was started in.
    /// If they differ the session is stopped when it is idle (the next delivery
    /// starts it again in the right place, as a new session); in a turn it is
    /// left alone and looked at again when the turn ends.
    pub(super) async fn realign_orchestrator(&mut self, chat: &str) {
        let key = orchestrator_session(chat);
        self.orchestrators.stale_cwd.remove(chat);
        if !self.sessions.is_live(&key) {
            return; // not running, or being started (it resolves its own directory)
        }
        let Some(branch) = self.state.chat(chat).map(|c| c.branch.clone()) else {
            return;
        };
        let Some(started_in) = self.stored_session(chat).await.and_then(|s| s.cwd) else {
            return;
        };
        let resolved = match self.git.resolve_branch_worktree(&branch).await {
            Ok(dir) => dir,
            Err(e) => {
                tracing::warn!("cannot find the working tree of {branch}: {e}");
                return;
            }
        };
        if Path::new(&started_in) == resolved {
            return;
        }
        if self.sessions.is_idle(&key) {
            tracing::info!(
                "chat {chat}: {branch} is now in {}, not {started_in}; restarting its orchestrator",
                resolved.display()
            );
            self.sessions.stop(&key);
        } else {
            self.orchestrators.stale_cwd.insert(chat.to_string());
        }
    }
}
