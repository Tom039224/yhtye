//! PR / merge status of each branch for the sidebar's BRANCHES tree: whether a
//! branch is still unmerged, has an open PR, or was merged (with or without
//! later changes). Read once from `gh` for all branches; `unknown` when `gh`
//! cannot tell (missing, unauthenticated, not a GitHub remote, or an error).

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::repo;
use super::run::git;

/// Longest the single `gh pr list` call may take (it uses the network).
const GH_TIMEOUT: Duration = Duration::from_secs(20);

/// What the dot left of a branch in the BRANCHES tree says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum BranchState {
    /// No PR, and not merged into the default branch.
    Unmerged,
    /// An open PR exists (drafts included).
    PrOpen,
    /// Merged by a PR, with no changes since (the branch HEAD is the PR head
    /// or its ancestor), or merged without any PR.
    Merged,
    /// Merged by a PR, but with commits past the PR head.
    MergedWithChanges,
    /// Cannot tell (`gh` missing / unauthenticated / not GitHub / failed).
    Unknown,
}

/// The PR / merge status of one branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct BranchStatus {
    pub branch: String,
    pub state: BranchState,
    /// The PR the state comes from (`None`: no PR, or unknown).
    pub pr_number: Option<u32>,
    pub pr_url: Option<String>,
}

/// The statuses of a repository's branches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct BranchStatuses {
    /// What merges are measured against (`origin`'s HEAD, else `main` /
    /// `master` / the checked-out branch; `None`: no branch at all).
    pub default_branch: Option<String>,
    pub statuses: Vec<BranchStatus>,
}

/// One row of `gh pr list --json ...` (`state` is `OPEN`, `MERGED` or `CLOSED`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhPr {
    pub number: u32,
    pub state: String,
    #[serde(default)]
    pub head_ref_name: String,
    #[serde(default)]
    pub head_ref_oid: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub merged_at: Option<String>,
}

impl GhPr {
    fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case("open")
    }

    fn is_merged(&self) -> bool {
        self.state.eq_ignore_ascii_case("merged")
    }
}

/// Parses the JSON of `gh pr list` into PRs by their head branch.
pub fn group_prs(text: &str) -> Result<HashMap<String, Vec<GhPr>>, String> {
    let prs: Vec<GhPr> =
        serde_json::from_str(text).map_err(|e| format!("could not parse `gh pr list`: {e}"))?;
    let mut by_branch: HashMap<String, Vec<GhPr>> = HashMap::new();
    for pr in prs {
        by_branch.entry(pr.head_ref_name.clone()).or_default().push(pr);
    }
    Ok(by_branch)
}

/// Picks the state of a branch with `prs` PRs: an open PR wins, then the
/// latest merged one (by `merged_at`, then number); a merely closed PR counts
/// as no PR. `head_is_ancestor_of_pr_head` tells whether the branch HEAD is
/// the merged PR's head or older (no changes since the merge).
pub fn decide_state(
    prs: &[GhPr],
    branch_head: &str,
    merged_into_default: bool,
    head_is_ancestor_of_pr_head: bool,
) -> (BranchState, Option<u32>, Option<String>) {
    if let Some(pr) = prs.iter().filter(|p| p.is_open()).min_by_key(|p| p.number) {
        return (BranchState::PrOpen, Some(pr.number), pr.url.clone());
    }
    let mut merged: Vec<&GhPr> = prs.iter().filter(|p| p.is_merged()).collect();
    merged.sort_by(|a, b| {
        (a.merged_at.clone(), a.number).cmp(&(b.merged_at.clone(), b.number))
    });
    if let Some(pr) = merged.last() {
        if branch_head == pr.head_ref_oid || head_is_ancestor_of_pr_head {
            return (BranchState::Merged, Some(pr.number), pr.url.clone());
        }
        return (
            BranchState::MergedWithChanges,
            Some(pr.number),
            pr.url.clone(),
        );
    }
    if merged_into_default {
        (BranchState::Merged, None, None)
    } else {
        (BranchState::Unmerged, None, None)
    }
}

/// What merges are measured against: `origin`'s HEAD, else an existing `main` /
/// `master`, else the branch checked out now.
pub async fn default_branch(dir: &Path) -> Option<String> {
    if let Ok(out) = git(dir, &["symbolic-ref", "refs/remotes/origin/HEAD"]).await
        && out.ok
        && let Some(name) = out
            .stdout
            .trim()
            .strip_prefix("refs/remotes/origin/")
    {
        return Some(name.to_string());
    }
    for name in ["main", "master"] {
        if repo::branch_exists(dir, name).await.unwrap_or(false) {
            return Some(name.to_string());
        }
    }
    repo::current_branch(dir).await.ok().flatten()
}

/// The local branches with their commit ids, by name.
async fn local_branches(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let out = git(
        dir,
        &["for-each-ref", "--format=%(refname:short) %(objectname)", "refs/heads"],
    )
    .await?;
    let text = out.into_result("git for-each-ref")?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let (name, sha) = l.split_once(' ')?;
            (!name.is_empty() && !sha.trim().is_empty())
                .then(|| (name.to_string(), sha.trim().to_string()))
        })
        .collect())
}

/// One `gh pr list` for the whole repository (`None`: `gh` failed, so every
/// branch is `unknown`).
async fn fetch_prs(dir: &Path) -> Option<HashMap<String, Vec<GhPr>>> {
    let mut cmd = tokio::process::Command::new("gh");
    cmd.args([
        "pr",
        "list",
        "--state",
        "all",
        "--limit",
        "200",
        "--json",
        "number,state,headRefName,headRefOid,mergedAt,url",
    ])
    .current_dir(dir)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .env("LC_ALL", "C")
    .env("GH_PROMPT_DISABLED", "1");
    let output = tokio::time::timeout(GH_TIMEOUT, cmd.output()).await.ok()?.ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    group_prs(&text).ok()
}

/// The PR / merge status of every local branch. Never fails: when `gh` cannot
/// tell, every branch is `unknown`.
pub async fn branch_statuses(dir: &Path) -> BranchStatuses {
    let branches = local_branches(dir).await.unwrap_or_default();
    let default = default_branch(dir).await;
    // A failed `gh` call is an empty listing with `failed`: every branch is
    // `unknown` rather than `unmerged` (which an empty listing also gives).
    let fetched = fetch_prs(dir).await;
    let failed = fetched.is_none();
    let prs = fetched.unwrap_or_default();
    let mut statuses = Vec::with_capacity(branches.len());
    for (branch, sha) in &branches {
        let list = prs.get(branch).map(Vec::as_slice).unwrap_or(&[]);
        let merged_into_default = match &default {
            Some(d) if d != branch => repo::is_ancestor(dir, branch, d).await.unwrap_or(false),
            _ => false,
        };
        // The ancestor check against the merged PR's head (whether the branch
        // has commits past it); only asked when a merged PR exists.
        let mut ancestor_of_pr_head = false;
        if let Some(pr) = latest_merged(list)
            && sha != &pr.head_ref_oid
        {
            ancestor_of_pr_head = repo::is_ancestor(dir, sha, &pr.head_ref_oid)
                .await
                .unwrap_or(false);
        }
        let (state, pr_number, pr_url) = if failed {
            (BranchState::Unknown, None, None)
        } else {
            decide_state(list, sha, merged_into_default, ancestor_of_pr_head)
        };
        statuses.push(BranchStatus {
            branch: branch.clone(),
            state,
            pr_number,
            pr_url,
        });
    }
    statuses.sort_by(|a, b| a.branch.cmp(&b.branch));
    BranchStatuses {
        default_branch: default,
        statuses,
    }
}

/// The merged PR the state is measured against (latest `merged_at`, then number).
fn latest_merged(prs: &[GhPr]) -> Option<&GhPr> {
    let mut merged: Vec<&GhPr> = prs.iter().filter(|p| p.is_merged()).collect();
    merged.sort_by(|a, b| {
        (a.merged_at.clone(), a.number).cmp(&(b.merged_at.clone(), b.number))
    });
    merged.into_iter().last()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(number: u32, state: &str, head: &str, oid: &str, merged_at: Option<&str>) -> GhPr {
        GhPr {
            number,
            state: state.into(),
            head_ref_name: head.into(),
            head_ref_oid: oid.into(),
            url: Some(format!("https://github.com/u/r/pull/{number}")),
            merged_at: merged_at.map(str::to_string),
        }
    }

    #[test]
    fn gh_output_is_grouped_by_head_branch() {
        let text = serde_json::json!([
            {"number": 7, "state": "OPEN", "headRefName": "feat/x", "headRefOid": "a", "mergedAt": null, "url": "u7"},
            {"number": 3, "state": "MERGED", "headRefName": "feat/x", "headRefOid": "b", "mergedAt": "2026-01-01T00:00:00Z", "url": "u3"},
            {"number": 5, "state": "CLOSED", "headRefName": "feat/y", "headRefOid": "c", "mergedAt": null, "url": "u5"}
        ])
        .to_string();
        let grouped = group_prs(&text).expect("parsed");
        assert_eq!(grouped["feat/x"].len(), 2);
        assert_eq!(grouped["feat/y"].len(), 1);
        assert!(group_prs("not json").is_err());
    }

    #[test]
    fn no_pr_means_unmerged_unless_merged_into_the_default_branch() {
        assert_eq!(
            decide_state(&[], "sha", false, false).0,
            BranchState::Unmerged
        );
        let (state, number, url) = decide_state(&[], "sha", true, false);
        assert_eq!((state, number, url), (BranchState::Merged, None, None));
    }

    #[test]
    fn an_open_pr_wins_over_a_merged_one() {
        let prs = vec![
            pr(3, "MERGED", "feat/x", "b", Some("2026-01-02T00:00:00Z")),
            pr(7, "OPEN", "feat/x", "a", None),
        ];
        let (state, number, _) = decide_state(&prs, "a", false, false);
        assert_eq!((state, number), (BranchState::PrOpen, Some(7)));
    }

    #[test]
    fn only_closed_prs_count_as_no_pr() {
        let prs = vec![pr(5, "CLOSED", "feat/y", "c", None)];
        assert_eq!(
            decide_state(&prs, "c", false, false).0,
            BranchState::Unmerged
        );
        assert_eq!(decide_state(&prs, "c", true, false).0, BranchState::Merged);
    }

    #[test]
    fn the_latest_merged_pr_decides() {
        let prs = vec![
            pr(3, "MERGED", "feat/x", "old", Some("2026-01-01T00:00:00Z")),
            pr(9, "MERGED", "feat/x", "new", Some("2026-02-01T00:00:00Z")),
        ];
        // At the PR head: merged.
        let (state, number, _) = decide_state(&prs, "new", false, false);
        assert_eq!((state, number), (BranchState::Merged, Some(9)));
        // An older HEAD that is the PR head's ancestor: still merged.
        let (state, number, _) = decide_state(&prs, "older", false, true);
        assert_eq!((state, number), (BranchState::Merged, Some(9)));
        // Past the PR head: changes since the merge.
        let (state, number, url) = decide_state(&prs, "later", false, false);
        assert_eq!(state, BranchState::MergedWithChanges);
        assert_eq!(number, Some(9));
        assert!(url.is_some());
    }

    /// The ancestor check against a real repository: a merged branch is its
    /// default branch's ancestor, an unmerged one is not.
    #[tokio::test]
    async fn ancestor_checks_read_git() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("run git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.com"]);
        std::fs::write(dir.path().join("f"), "1").expect("write");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "initial"]);
        git(&["branch", "merged"]);
        git(&["checkout", "-q", "-b", "unmerged"]);
        std::fs::write(dir.path().join("f"), "2").expect("write");
        git(&["commit", "-qam", "more"]);
        assert_eq!(default_branch(dir.path()).await.as_deref(), Some("main"));
        assert!(repo::is_ancestor(dir.path(), "merged", "main").await.expect("ancestor"));
        assert!(!repo::is_ancestor(dir.path(), "unmerged", "main").await.expect("ancestor"));
    }

    #[tokio::test]
    async fn the_default_branch_falls_back_without_a_remote() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("run git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "dev"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.com"]);
        std::fs::write(dir.path().join("f"), "1").expect("write");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "initial"]);
        assert_eq!(default_branch(dir.path()).await.as_deref(), Some("dev"));
    }
}
