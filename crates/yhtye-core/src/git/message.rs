//! Messages of the commits Yhtye makes on its integration branches: one squash
//! commit per `code` task on the group branch, and the merge commit of the group
//! on the base branch (`orchestration-model.md` §6). Pure text; the git side
//! gathers the inputs.
//!
//! Both end with the "Created with Yhtye" line and a trailer paragraph
//! (`Yhtye-Group:` ...) that `git interpret-trailers --parse` and
//! `git log --format=%(trailers)` read, so the trailers are always the last
//! paragraph.

use crate::agents::AgentChoice;
use crate::domain::GroupTask;

/// The version in the footer line.
///
/// A fixed `0.0.0` for now, not `CARGO_PKG_VERSION`. TODO.md (item under
/// "0.1.0 リリース時") tracks making it follow the release version automatically.
pub const CREATED_WITH_VERSION: &str = "0.0.0";

const CREATED_WITH_URL: &str = "github.com/Tom039224/yhtye";

/// `♪ Created with Yhtye v<version>: github.com/Tom039224/yhtye`, the footer
/// line of every commit message Yhtye writes.
#[must_use]
pub fn created_with_line() -> String {
    format!("♪ Created with Yhtye v{CREATED_WITH_VERSION}: {CREATED_WITH_URL}")
}

/// What the squash commit of a task says.
pub struct SquashMessage<'a> {
    pub group: &'a str,
    pub task: &'a str,
    pub title: &'a str,
    /// Subjects of the commits on the task branch (oldest first).
    pub commits: &'a [String],
    /// The agent that implemented the task, if known.
    pub implementer: Option<&'a AgentChoice>,
    /// The agents of the task's review steps (empty: it had no review step).
    pub reviewers: &'a [AgentChoice],
}

/// The message of the squash commit that merges a task into its group branch:
/// the task's title, a bullet per commit, the footer line, then the trailers.
#[must_use]
pub fn squash_message(m: &SquashMessage<'_>) -> String {
    let title = one_line(m.title);
    let mut paragraphs = vec![if title.is_empty() {
        m.task.to_string()
    } else {
        title
    }];
    if !m.commits.is_empty() {
        let bullets: Vec<String> = m.commits.iter().map(|c| format!("- {c}")).collect();
        paragraphs.push(bullets.join("\n"));
    }
    paragraphs.push(created_with_line());
    let mut trailers = vec![
        format!("Yhtye-Group: {}", m.group),
        format!("Yhtye-Task: {}", m.task),
    ];
    trailers.extend(
        m.implementer
            .and_then(agent_label)
            .map(|a| format!("Yhtye-Agent: {a}")),
    );
    trailers.extend(
        m.reviewers
            .iter()
            .filter_map(agent_label)
            .map(|a| format!("Yhtye-Reviewer: {a}")),
    );
    paragraphs.push(trailers.join("\n"));
    paragraphs.join("\n\n")
}

/// What the merge commit of a group on its base branch says.
pub struct GroupMessage<'a> {
    pub group: &'a str,
    pub group_branch: &'a str,
    pub base_branch: &'a str,
    pub title: &'a str,
    /// `finish_group`'s summary (empty: none).
    pub summary: &'a str,
    /// The tasks merged into the group, in merge order.
    pub tasks: &'a [GroupTask],
}

/// The message of the merge commit of a group on its base branch.
#[must_use]
pub fn group_message(m: &GroupMessage<'_>) -> String {
    let mut paragraphs = vec![format!("Merge {} into {}", m.group_branch, m.base_branch)];
    let title = one_line(m.title);
    if !title.is_empty() {
        paragraphs.push(title);
    }
    let summary = m.summary.trim();
    if !summary.is_empty() {
        paragraphs.push(summary.to_string());
    }
    if !m.tasks.is_empty() {
        let lines: Vec<String> = m
            .tasks
            .iter()
            .map(|t| format!("- {}: {}", t.id, one_line(&t.title)))
            .collect();
        paragraphs.push(lines.join("\n"));
    }
    paragraphs.push(created_with_line());
    paragraphs.push(format!("Yhtye-Group: {}", m.group));
    paragraphs.join("\n\n")
}

/// `tasks` in the order their ids appear in `merged` (oldest merge first);
/// tasks not in `merged` keep their own order after those.
#[must_use]
pub fn in_merge_order(tasks: &[GroupTask], merged: &[String]) -> Vec<GroupTask> {
    let mut ordered = tasks.to_vec();
    ordered.sort_by_key(|t| merged.iter().position(|m| *m == t.id).unwrap_or(usize::MAX));
    ordered
}

/// `harness/model/effort`, leaving out what is not set; `None` if nothing is.
fn agent_label(agent: &AgentChoice) -> Option<String> {
    let parts: Vec<&str> = [
        Some(agent.harness.as_str()),
        agent.model.as_deref(),
        agent.effort.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|p| !p.is_empty())
    .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `text` on one line (a title must not break the message's layout).
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, title: &str) -> GroupTask {
        GroupTask {
            id: id.into(),
            title: title.into(),
        }
    }

    fn squash<'a>(commits: &'a [String], reviewers: &'a [AgentChoice]) -> SquashMessage<'a> {
        SquashMessage {
            group: "api",
            task: "login",
            title: "Add the login endpoint",
            commits,
            implementer: None,
            reviewers,
        }
    }

    #[test]
    fn the_footer_names_the_fixed_version() {
        assert_eq!(
            created_with_line(),
            "♪ Created with Yhtye v0.0.0: github.com/Tom039224/yhtye"
        );
    }

    #[test]
    fn a_squash_message_has_title_bullets_footer_and_trailers_last() {
        let commits = ["add route".to_string(), "add tests".to_string()];
        let reviewers = [AgentChoice::new("claude-code", Some("opus")).with_effort("high")];
        let implementer = AgentChoice::new("codex", Some("gpt-5"));
        let msg = squash_message(&SquashMessage {
            implementer: Some(&implementer),
            ..squash(&commits, &reviewers)
        });
        assert_eq!(
            msg,
            "Add the login endpoint\n\
             \n\
             - add route\n\
             - add tests\n\
             \n\
             ♪ Created with Yhtye v0.0.0: github.com/Tom039224/yhtye\n\
             \n\
             Yhtye-Group: api\n\
             Yhtye-Task: login\n\
             Yhtye-Agent: codex/gpt-5\n\
             Yhtye-Reviewer: claude-code/opus/high"
        );
    }

    #[test]
    fn a_squash_message_without_commits_or_agents_has_no_bullets_or_agent_trailers() {
        let msg = squash_message(&squash(&[], &[]));
        assert_eq!(
            msg,
            "Add the login endpoint\n\
             \n\
             ♪ Created with Yhtye v0.0.0: github.com/Tom039224/yhtye\n\
             \n\
             Yhtye-Group: api\n\
             Yhtye-Task: login"
        );
    }

    #[test]
    fn agent_labels_leave_out_what_is_missing() {
        let harness_only = AgentChoice::new("opencode", None);
        let no_model = AgentChoice {
            effort: Some("low".into()),
            ..AgentChoice::new("opencode", None)
        };
        let nothing = AgentChoice::new("", None);
        assert_eq!(agent_label(&harness_only).as_deref(), Some("opencode"));
        assert_eq!(agent_label(&no_model).as_deref(), Some("opencode/low"));
        assert_eq!(agent_label(&nothing), None);
    }

    #[test]
    fn a_title_with_line_breaks_stays_one_line() {
        let commits = [];
        let msg = squash_message(&SquashMessage {
            title: "  Add\nlogin\n\nendpoint ",
            ..squash(&commits, &[])
        });
        assert!(msg.starts_with("Add login endpoint\n\n♪ Created"), "{msg}");
        let empty = squash_message(&SquashMessage {
            title: "  ",
            ..squash(&commits, &[])
        });
        assert!(
            empty.starts_with("login\n\n"),
            "falls back to the id: {empty}"
        );
    }

    #[test]
    fn a_group_message_lists_title_summary_tasks_footer_and_trailer() {
        let tasks = [task("login", "Add login"), task("logout", "Add\nlogout")];
        let msg = group_message(&GroupMessage {
            group: "api",
            group_branch: "yhtye/api",
            base_branch: "main",
            title: "Authentication",
            summary: "  Users can sign in and out.\n",
            tasks: &tasks,
        });
        assert_eq!(
            msg,
            "Merge yhtye/api into main\n\
             \n\
             Authentication\n\
             \n\
             Users can sign in and out.\n\
             \n\
             - login: Add login\n\
             - logout: Add logout\n\
             \n\
             ♪ Created with Yhtye v0.0.0: github.com/Tom039224/yhtye\n\
             \n\
             Yhtye-Group: api"
        );
    }

    #[test]
    fn a_group_message_leaves_out_an_empty_summary_and_task_list() {
        let msg = group_message(&GroupMessage {
            group: "g1",
            group_branch: "yhtye/g1",
            base_branch: "main",
            title: "Title",
            summary: " ",
            tasks: &[],
        });
        assert_eq!(
            msg,
            "Merge yhtye/g1 into main\n\
             \n\
             Title\n\
             \n\
             ♪ Created with Yhtye v0.0.0: github.com/Tom039224/yhtye\n\
             \n\
             Yhtye-Group: g1"
        );
    }

    #[test]
    fn tasks_are_listed_in_the_order_they_were_merged() {
        let tasks = [
            task("a", "A"),
            task("b", "B"),
            task("c", "C"),
            task("d", "D"),
        ];
        let merged = ["c".to_string(), "other".to_string(), "a".to_string()];
        let ids: Vec<String> = in_merge_order(&tasks, &merged)
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(
            ids,
            ["c", "a", "b", "d"],
            "unmerged ones follow in their own order"
        );
    }
}
