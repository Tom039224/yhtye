//! A read-only view of a repository for the UI's git panel (Stage 6a): the
//! checked-out branch, the local branches and the recent commit graph of all
//! local branches (`git log --topo-order`). Lanes are laid out by the UI.

use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use super::repo::current_branch;
use super::run::git;
use super::worktree::list;
use crate::domain::INTERNAL_BRANCH_PREFIX;

/// Field and record separators of the `git log` format (unit / record separator).
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

/// Most commits a [`GitOverview`] may carry.
pub const MAX_GRAPH_COMMITS: usize = 500;

/// The repository as the git panel shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct GitOverview {
    /// Branch checked out in the project directory (`None`: detached HEAD or no commits).
    pub head: Option<String>,
    /// Commit of `HEAD` (`None`: no commits yet).
    pub head_sha: Option<String>,
    /// Local branches, by name.
    pub branches: Vec<GitBranch>,
    /// Every worktree of the repository, the main one first (Stage 8e: the
    /// BRANCHES tree and the branch shown for a chat come from here).
    pub worktrees: Vec<GitWorktree>,
    /// Newest first, parents after their children (`--topo-order`).
    pub commits: Vec<GitCommit>,
    /// More commits exist beyond `commits`.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct GitBranch {
    pub name: String,
    pub sha: String,
}

/// One worktree as `git worktree list --porcelain` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct GitWorktree {
    /// As git shows it (what a chat's `worktree` is compared with).
    pub path: String,
    /// Checked-out branch (`None`: detached HEAD).
    pub branch: Option<String>,
    /// Checked-out commit (`None`: no commits yet).
    pub head_sha: Option<String>,
    /// The repository's main worktree (the original clone).
    pub is_main: bool,
    /// Its directory is gone.
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct GitCommit {
    pub sha: String,
    /// First parent first (the branch the commit was made on).
    pub parents: Vec<String>,
    /// Local branches pointing here.
    pub branches: Vec<String>,
    pub subject: String,
    /// Committer time.
    pub ts_ms: u64,
}

/// Reads the overview of the repository at `dir` with at most `limit` commits.
pub async fn overview(dir: &Path, limit: usize) -> Result<GitOverview, String> {
    let limit = limit.clamp(1, MAX_GRAPH_COMMITS);
    let head = current_branch(dir).await?;
    let head_sha = {
        let out = git(dir, &["rev-parse", "--verify", "-q", "HEAD^{commit}"]).await?;
        out.ok.then(|| out.stdout.trim().to_string())
    };
    let branches = branches(dir).await?;
    let worktrees = worktrees(dir).await?;
    let (commits, truncated) = if branches.is_empty() && head_sha.is_none() {
        (Vec::new(), false)
    } else {
        log(dir, limit, head_sha.is_some()).await?
    };
    Ok(GitOverview {
        head,
        head_sha,
        branches,
        worktrees,
        commits,
        truncated,
    })
}

async fn worktrees(dir: &Path) -> Result<Vec<GitWorktree>, String> {
    let all = list(dir).await?;
    Ok(all
        .into_iter()
        .enumerate()
        .map(|(i, w)| GitWorktree {
            missing: !w.path.is_dir(),
            path: w.path.to_string_lossy().into_owned(),
            branch: w.branch,
            head_sha: w.head,
            is_main: i == 0,
        })
        .collect())
}

/// The local branches a user works on: every branch but Yhtye's own `yhtye/*`.
pub async fn list_branches(dir: &Path) -> Result<Vec<GitBranch>, String> {
    let mut list = branches(dir).await?;
    list.retain(|b| !b.name.starts_with(INTERNAL_BRANCH_PREFIX));
    Ok(list)
}

async fn branches(dir: &Path) -> Result<Vec<GitBranch>, String> {
    let fmt = format!("--format=%(refname:short){FIELD}%(objectname)");
    let out = git(dir, &["for-each-ref", &fmt, "refs/heads"]).await?;
    let text = out.into_result("git for-each-ref")?;
    let mut list: Vec<GitBranch> = text
        .lines()
        .filter_map(|l| {
            let (name, sha) = l.split_once(FIELD)?;
            Some(GitBranch {
                name: name.to_string(),
                sha: sha.trim().to_string(),
            })
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

async fn log(dir: &Path, limit: usize, with_head: bool) -> Result<(Vec<GitCommit>, bool), String> {
    let fmt = format!("--format=%H{FIELD}%P{FIELD}%D{FIELD}%ct{FIELD}%s{RECORD}");
    let count = format!("--max-count={}", limit + 1);
    let mut args = vec![
        "log",
        "--topo-order",
        "--decorate=full",
        "--decorate-refs=refs/heads/",
        &count,
        &fmt,
        "--branches",
    ];
    // HEAD may be detached (not on a branch), or unborn (no commits yet).
    if with_head {
        args.push("HEAD");
    }
    let out = git(dir, &args).await?;
    let text = out.into_result("git log")?;
    let mut commits: Vec<GitCommit> = text
        .split(RECORD)
        .filter_map(parse_commit)
        .collect::<Vec<_>>();
    let truncated = commits.len() > limit;
    commits.truncate(limit);
    Ok((commits, truncated))
}

fn parse_commit(record: &str) -> Option<GitCommit> {
    let record = record.trim_start_matches('\n');
    if record.trim().is_empty() {
        return None;
    }
    let mut f = record.splitn(5, FIELD);
    let sha = f.next()?.trim().to_string();
    let parents = f
        .next()?
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let branches = parse_decorations(f.next()?);
    let ts_ms = f.next()?.trim().parse::<u64>().ok()? * 1000;
    let subject = f.next().unwrap_or("").trim_end_matches('\n').to_string();
    Some(GitCommit {
        sha,
        parents,
        branches,
        subject,
        ts_ms,
    })
}

/// `HEAD -> refs/heads/main, refs/heads/yhtye/G-1` → `["main", "yhtye/G-1"]`.
fn parse_decorations(text: &str) -> Vec<String> {
    text.split(", ")
        .map(|d| d.trim().trim_start_matches("HEAD -> "))
        .filter_map(|d| d.strip_prefix("refs/heads/"))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations_keep_local_branches_only() {
        assert_eq!(
            parse_decorations("HEAD -> refs/heads/main, refs/heads/yhtye/G-1-T-2"),
            vec!["main", "yhtye/G-1-T-2"]
        );
        assert_eq!(parse_decorations("HEAD"), Vec::<String>::new());
        assert_eq!(parse_decorations(""), Vec::<String>::new());
    }

    #[test]
    fn commit_records_are_parsed() {
        let rec =
            format!("\nabc{FIELD}p1 p2{FIELD}refs/heads/x{FIELD}1700000000{FIELD}merge: a\u{1f}b");
        let c = parse_commit(&rec).expect("parsed");
        assert_eq!(c.sha, "abc");
        assert_eq!(c.parents, vec!["p1", "p2"]);
        assert_eq!(c.branches, vec!["x"]);
        assert_eq!(c.ts_ms, 1_700_000_000_000);
        // The subject is the rest of the record, separators included.
        assert_eq!(c.subject, "merge: a\u{1f}b");
        assert!(parse_commit("\n").is_none());
    }

    #[tokio::test]
    async fn an_empty_repository_has_no_commits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let init = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .expect("git init");
        assert!(init.success());
        let o = overview(dir.path(), 10).await.expect("overview");
        assert_eq!(o.head.as_deref(), Some("main"));
        assert_eq!(o.head_sha, None);
        assert!(o.branches.is_empty() && o.commits.is_empty() && !o.truncated);
    }
}
