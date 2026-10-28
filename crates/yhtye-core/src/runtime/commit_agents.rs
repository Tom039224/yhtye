//! The agents named in a task's squash commit (`Yhtye-Agent:` /
//! `Yhtye-Reviewer:`): the harness × model × effort their sessions were started
//! with, as the session records hold it. The state machine does not know them
//! (it only has the task's optional override), so the runtime fills them into
//! the finish operation before git runs it.

use super::driver::Driver;
use crate::agents::AgentChoice;
use crate::domain::{GitOp, Role, TaskKind};
use crate::store::SessionRecord;

impl Driver {
    /// `op` with the agents of the task filled in, if it finishes a `code` task
    /// (else unchanged). Agents that cannot be read are left out of the commit.
    pub(super) async fn with_commit_agents(&mut self, mut op: GitOp) -> GitOp {
        let GitOp::FinishTask {
            kind: TaskKind::Code,
            task,
            agent,
            reviewers,
            ..
        } = &mut op
        else {
            return op;
        };
        self.publisher.flush().await;
        match self.publisher.store().sessions(&self.cfg.project).await {
            Ok(all) => (*agent, *reviewers) = task_agents(&all, task),
            Err(e) => tracing::warn!("could not read the agents of {task} for its commit: {e}"),
        }
        op
    }
}

/// The agent that implemented `task` and the agents of its review steps (oldest
/// step first, each once), from the recorded sessions.
fn task_agents(sessions: &[SessionRecord], task: &str) -> (Option<AgentChoice>, Vec<AgentChoice>) {
    let of_task = || sessions.iter().filter(|s| s.task.as_deref() == Some(task));
    let implementer = of_task()
        .find(|s| s.role == Role::Implementer)
        .and_then(|s| s.agent.clone());
    let review_prefix = format!("{task}/review-");
    let mut reviews: Vec<(usize, &AgentChoice)> = of_task()
        .filter(|s| s.role == Role::Reviewer)
        .filter_map(|s| {
            let step = s.session_key.strip_prefix(&review_prefix)?.parse().ok()?;
            Some((step, s.agent.as_ref()?))
        })
        .collect();
    reviews.sort_by_key(|(step, _)| *step);
    let mut reviewers: Vec<AgentChoice> = Vec::new();
    for (_, agent) in reviews {
        if !reviewers.contains(agent) {
            reviewers.push(agent.clone());
        }
    }
    (implementer, reviewers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SessionStatus;

    fn record(key: &str, role: Role, task: &str, agent: Option<AgentChoice>) -> SessionRecord {
        SessionRecord {
            session_key: key.into(),
            role,
            task: Some(task.into()),
            acp_session_id: "acp".into(),
            status: SessionStatus::Stopped,
            turn_running: false,
            agent,
            cwd: None,
            lost_acp_session_id: None,
        }
    }

    fn cc(model: &str) -> AgentChoice {
        AgentChoice::new("claude-code", Some(model))
    }

    #[test]
    fn the_task_s_implementer_and_distinct_reviewers_come_from_its_sessions() {
        let sessions = [
            record(
                "other/implementer",
                Role::Implementer,
                "other",
                Some(cc("haiku")),
            ),
            record("t/implementer", Role::Implementer, "t", Some(cc("sonnet"))),
            record("t/review-3", Role::Reviewer, "t", Some(cc("opus"))),
            record("t/review-1", Role::Reviewer, "t", Some(cc("fable"))),
            record("t/review-5", Role::Reviewer, "t", Some(cc("opus"))),
            record("t/review-7", Role::Reviewer, "t", None),
            record("t2/review-1", Role::Reviewer, "t2", Some(cc("haiku"))),
        ];
        let (agent, reviewers) = task_agents(&sessions, "t");
        assert_eq!(agent, Some(cc("sonnet")));
        assert_eq!(reviewers, [cc("fable"), cc("opus")]);
    }

    #[test]
    fn a_task_without_sessions_names_no_agent() {
        assert_eq!(task_agents(&[], "t"), (None, Vec::new()));
        let sessions = [record("t/implementer", Role::Implementer, "t", None)];
        assert_eq!(task_agents(&sessions, "t"), (None, Vec::new()));
    }
}
