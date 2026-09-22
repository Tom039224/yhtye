//! Temporary git repositories for tests. Everything lives under one temp
//! directory: the repository (`repo/`) and Yhtye's data directory (`data/`,
//! database and worktrees), so the repository's tree only holds what the test
//! puts there. Git never runs in the Yhtye repository itself.

use std::path::{Path, PathBuf};
use std::process::Command;

use yhtye_core::git::{GitCli, worktree_root};

pub const PROJECT: &str = "P-1";

pub struct TempRepo {
    _root: tempfile::TempDir,
    pub repo: PathBuf,
    pub data: PathBuf,
}

impl TempRepo {
    /// A repository on `main` with one commit (`README.md`).
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let repo = root.path().join("repo");
        let data = root.path().join("data");
        std::fs::create_dir_all(&repo).expect("mkdir repo");
        std::fs::create_dir_all(&data).expect("mkdir data");
        let r = Self {
            _root: root,
            repo,
            data,
        };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["config", "user.name", "Test"]);
        r.git(&["config", "user.email", "test@example.com"]);
        r.write("README.md", "# test\n");
        r.commit("initial");
        r
    }

    pub fn git_cli(&self) -> GitCli {
        GitCli::new(&self.repo, worktree_root(&self.data, PROJECT))
    }

    pub fn db(&self) -> PathBuf {
        yhtye_core::store::db_path(&self.data)
    }

    /// Runs git in the main worktree and returns stdout (panics on failure).
    pub fn git(&self, args: &[&str]) -> String {
        git_in(&self.repo, args)
    }

    /// Like [`TempRepo::git`], but returns whether it succeeded.
    pub fn git_status(&self, args: &[&str]) -> bool {
        command(&self.repo, args)
            .status()
            .expect("run git")
            .success()
    }

    pub fn write(&self, path: &str, text: &str) {
        write_in(&self.repo, path, text);
    }

    pub fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.repo.join(path)).expect("read file")
    }

    pub fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// `path` at `rev` (e.g. a branch).
    pub fn show(&self, rev: &str, path: &str) -> String {
        self.git(&["show", &format!("{rev}:{path}")])
    }

    pub fn branch_exists(&self, branch: &str) -> bool {
        self.git_status(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
    }

    /// Paths of the repository's worktrees other than the main one.
    pub fn extra_worktrees(&self) -> Vec<String> {
        self.git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .skip(1)
            .map(str::to_string)
            .collect()
    }

    /// `git status --porcelain` of the main worktree.
    pub fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
        cmd.env_remove(var);
    }
    cmd
}

/// Runs git in `dir` and returns stdout (panics on failure).
pub fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = command(dir, args).output().expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn write_in(dir: &Path, path: &str, text: &str) {
    let full = dir.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(full, text).expect("write file");
}
