//! The app identifier changed from `com.tom039224.yhtye` to
//! `io.github.tom039224.yhtye`, which moves the app data directory. On the
//! first start after the change the old directory is renamed to the new one so
//! existing users keep their database, WebKit storage and git worktrees.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Identifier (and so data directory name) used before the rename.
pub const LEGACY_IDENTIFIER: &str = "com.tom039224.yhtye";

/// What [`migrate`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The legacy directory was renamed to the new one.
    Moved,
    /// There is no legacy directory (fresh install, or already migrated).
    NoLegacy,
    /// Both directories exist: leave them alone rather than merge.
    NewExists,
    /// The rename failed (e.g. across devices); the app starts with an empty dir.
    RenameFailed,
}

/// Renames the sibling `com.tom039224.yhtye` directory to `new_dir` when only
/// the legacy one exists, then repairs the git worktrees inside it. Never fails.
pub fn migrate(new_dir: &Path) -> Outcome {
    let Some(legacy) = new_dir.parent().map(|p| p.join(LEGACY_IDENTIFIER)) else {
        return Outcome::NoLegacy;
    };
    if !legacy.exists() {
        tracing::info!("no legacy data dir at {}", legacy.display());
        return Outcome::NoLegacy;
    }
    if new_dir.exists() {
        tracing::warn!(
            "both {} and {} exist; not migrating",
            legacy.display(),
            new_dir.display()
        );
        return Outcome::NewExists;
    }
    if let Err(e) = std::fs::rename(&legacy, new_dir) {
        tracing::warn!(
            "could not move {} to {}: {e}",
            legacy.display(),
            new_dir.display()
        );
        return Outcome::RenameFailed;
    }
    tracing::info!("moved {} to {}", legacy.display(), new_dir.display());
    repair_worktrees(new_dir);
    Outcome::Moved
}

/// Runs `git worktree repair` inside every `worktrees/*/*/*` directory that has
/// a `.git` file. The worktree's `.git` file still points at the repo's admin
/// directory, so repairing from inside fixes the repo's back-reference.
fn repair_worktrees(data_dir: &Path) {
    for worktree in worktree_dirs(&data_dir.join("worktrees")) {
        match Command::new("git")
            .args(["worktree", "repair"])
            .current_dir(&worktree)
            .output()
        {
            Ok(out) if out.status.success() => {}
            Ok(out) => tracing::warn!(
                "git worktree repair in {} failed: {}",
                worktree.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(e) => tracing::warn!("git worktree repair in {}: {e}", worktree.display()),
        }
    }
}

/// Directories at depth 3 under `root` (`<project>/<group>/<task>`) with a `.git` file.
fn worktree_dirs(root: &Path) -> Vec<PathBuf> {
    let children = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    };
    children(root)
        .iter()
        .flat_map(|p| children(p))
        .flat_map(|g| children(&g))
        .filter(|t| t.join(".git").is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn paths(root: &Path) -> (PathBuf, PathBuf) {
        (
            root.join(LEGACY_IDENTIFIER),
            root.join("io.github.tom039224.yhtye"),
        )
    }

    #[test]
    fn moves_when_only_legacy_exists() {
        let root = tempfile::tempdir().unwrap();
        let (legacy, new) = paths(root.path());
        fs::create_dir_all(legacy.join("sub")).unwrap();
        fs::write(legacy.join("sub/yhtye.sqlite3"), b"data").unwrap();

        assert_eq!(migrate(&new), Outcome::Moved);
        assert!(!legacy.exists());
        assert_eq!(fs::read(new.join("sub/yhtye.sqlite3")).unwrap(), b"data");
    }

    #[test]
    fn does_nothing_when_new_exists() {
        let root = tempfile::tempdir().unwrap();
        let (legacy, new) = paths(root.path());
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(legacy.join("a"), b"old").unwrap();

        assert_eq!(migrate(&new), Outcome::NewExists);
        assert!(legacy.join("a").exists());
        assert!(!new.join("a").exists());
    }

    #[test]
    fn does_nothing_when_neither_exists() {
        let root = tempfile::tempdir().unwrap();
        let (_, new) = paths(root.path());

        assert_eq!(migrate(&new), Outcome::NoLegacy);
        assert!(!new.exists());
    }

    #[test]
    fn moved_git_worktree_keeps_working() {
        let root = tempfile::tempdir().unwrap();
        let (legacy, new) = paths(root.path());
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let old_wt = legacy.join("worktrees/proj/group/task");
        fs::create_dir_all(old_wt.parent().unwrap()).unwrap();
        git(
            &repo,
            &["worktree", "add", "-q", "-b", "t", old_wt.to_str().unwrap()],
        );

        assert_eq!(migrate(&new), Outcome::Moved);

        let new_wt = new.join("worktrees/proj/group/task");
        let list = git(&repo, &["worktree", "list", "--porcelain"]);
        let new_wt_real = fs::canonicalize(&new_wt).unwrap();
        assert!(
            list.contains(new_wt_real.to_str().unwrap()),
            "worktree list: {list}"
        );
        git(&new_wt, &["status"]);
    }
}
