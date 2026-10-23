//! The folders of the host (`list_directory`): lets a remote client — the PC
//! app connected to another host, the Android app — browse the machine the core
//! runs on when it adds a project. A local file manager would answer with
//! paths of the wrong machine.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;
use ts_rs::TS;

use crate::api::{ApiError, ApiErrorCode};

/// A folder inside the listed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct DirectoryEntry {
    pub name: String,
    /// The folder's absolute path, to pass back as `path`.
    pub path: String,
    /// It contains `.git` (a directory, or a file in a linked worktree).
    pub is_git_repo: bool,
}

/// The folders (not files) of one directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct DirectoryListing {
    /// The listed directory, absolute with symbolic links resolved.
    pub path: String,
    /// Its parent; `None` for the root.
    pub parent: Option<String>,
    /// The home directory of the user the core runs as.
    pub home: String,
    /// The listed directory is itself a git repository.
    pub is_git_repo: bool,
    /// Sorted by name, ignoring case.
    pub entries: Vec<DirectoryEntry>,
}

/// The home directory of the user the core runs as (`HOME`, or `USERPROFILE`).
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|p| p.is_absolute())
}

fn io_error(path: &Path, e: &std::io::Error) -> ApiError {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => ApiErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => ApiErrorCode::Forbidden,
        std::io::ErrorKind::NotADirectory => ApiErrorCode::InvalidArgument,
        _ => ApiErrorCode::Internal,
    };
    let what = match e.kind() {
        std::io::ErrorKind::NotFound => "does not exist".to_string(),
        std::io::ErrorKind::PermissionDenied => "cannot be read (permission denied)".to_string(),
        std::io::ErrorKind::NotADirectory => "is not a directory".to_string(),
        _ => format!("cannot be read: {e}"),
    };
    ApiError::new(code, format!("{} {what}", path.display()))
}

/// `path` as the directory to list: `None` or empty is the home directory,
/// `~` and `~/…` start at it, anything else must be absolute.
fn requested(path: Option<&str>, home: &Path) -> Result<PathBuf, ApiError> {
    let path = path.map(str::trim).filter(|p| !p.is_empty());
    let Some(path) = path else {
        return Ok(home.to_path_buf());
    };
    if path == "~" {
        return Ok(home.to_path_buf());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(home.join(rest));
    }
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(ApiError::invalid_argument(format!(
            "{} is not an absolute path",
            path.display()
        )))
    }
}

/// The folder entry for `entry` when it is a directory (a symbolic link to one
/// counts) with a name and path that survive a round trip as text.
fn folder(entry: &std::fs::DirEntry, show_hidden: bool) -> Option<DirectoryEntry> {
    let name = entry.file_name().into_string().ok()?;
    if !show_hidden && name.starts_with('.') {
        return None;
    }
    let path = entry.path();
    let is_dir = match entry.file_type().ok()? {
        t if t.is_dir() => true,
        t if t.is_symlink() => std::fs::metadata(&path).is_ok_and(|m| m.is_dir()),
        _ => false,
    };
    if !is_dir {
        return None;
    }
    let is_git_repo = path.join(".git").exists();
    Some(DirectoryEntry {
        name,
        path: path.into_os_string().into_string().ok()?,
        is_git_repo,
    })
}

/// Lists the folders of `path` (see [`requested`]). Unreadable entries and
/// those that are not folders are left out; hidden ones (`.name`) too unless
/// `show_hidden`. `home` is where an absent path starts.
pub fn list_directory(
    path: Option<&str>,
    show_hidden: bool,
    home: Option<&Path>,
) -> Result<DirectoryListing, ApiError> {
    let home = home.ok_or_else(|| ApiError::unavailable("the home directory is not known"))?;
    let wanted = requested(path, home)?;
    let dir = std::fs::canonicalize(&wanted).map_err(|e| io_error(&wanted, &e))?;
    if !dir.is_dir() {
        return Err(ApiError::invalid_argument(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let mut entries: Vec<DirectoryEntry> = std::fs::read_dir(&dir)
        .map_err(|e| io_error(&dir, &e))?
        .flatten()
        .filter_map(|entry| folder(&entry, show_hidden))
        .collect();
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    let text = |p: &Path| {
        p.to_str()
            .map(str::to_string)
            .ok_or_else(|| ApiError::internal(format!("{} is not valid UTF-8", p.display())))
    };
    Ok(DirectoryListing {
        parent: dir.parent().map(text).transpose()?,
        is_git_repo: dir.join(".git").exists(),
        path: text(&dir)?,
        home: text(home)?,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(tmp: &Path) {
        for d in [
            "beta",
            "Alpha",
            "gamma/.git",
            ".hidden",
            "repo/.git",
            "zeta",
        ] {
            std::fs::create_dir_all(tmp.join(d)).unwrap();
        }
        std::fs::write(tmp.join("file.txt"), "x").unwrap();
        // A linked worktree has a `.git` file.
        std::fs::create_dir_all(tmp.join("linked")).unwrap();
        std::fs::write(tmp.join("linked/.git"), "gitdir: /x").unwrap();
    }

    fn names(l: &DirectoryListing) -> Vec<&str> {
        l.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn only_folders_are_listed_sorted_ignoring_case_with_their_git_flag() {
        let tmp = tempfile::tempdir().unwrap();
        make(tmp.path());
        let dir = tmp.path().canonicalize().unwrap();
        let l = list_directory(Some(dir.to_str().unwrap()), false, Some(&dir)).unwrap();
        assert_eq!(
            names(&l),
            ["Alpha", "beta", "gamma", "linked", "repo", "zeta"]
        );
        let git: Vec<&str> = l
            .entries
            .iter()
            .filter(|e| e.is_git_repo)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(git, ["gamma", "linked", "repo"]);
        assert_eq!(l.path, dir.to_str().unwrap());
        assert_eq!(l.parent.as_deref(), dir.parent().unwrap().to_str());
        assert!(!l.is_git_repo);
        assert_eq!(l.entries[0].path, dir.join("Alpha").to_str().unwrap());

        let all = list_directory(Some(dir.to_str().unwrap()), true, Some(&dir)).unwrap();
        assert_eq!(names(&all)[..2], [".hidden", "Alpha"]);

        let repo =
            list_directory(Some(dir.join("repo").to_str().unwrap()), false, Some(&dir)).unwrap();
        assert!(repo.is_git_repo && repo.entries.is_empty());
    }

    #[test]
    fn no_path_means_home_and_tilde_starts_there() {
        let tmp = tempfile::tempdir().unwrap();
        make(tmp.path());
        let home = tmp.path().canonicalize().unwrap();
        for path in [None, Some(""), Some("  "), Some("~")] {
            let l = list_directory(path, false, Some(&home)).unwrap();
            assert_eq!(l.path, home.to_str().unwrap());
            assert_eq!(l.home, home.to_str().unwrap());
        }
        let l = list_directory(Some("~/repo"), false, Some(&home)).unwrap();
        assert_eq!(l.path, home.join("repo").to_str().unwrap());
        assert_eq!(
            list_directory(None, false, None).unwrap_err().code,
            ApiErrorCode::Unavailable
        );
    }

    #[test]
    fn links_to_folders_are_followed_and_paths_are_made_canonical() {
        let tmp = tempfile::tempdir().unwrap();
        make(tmp.path());
        let dir = tmp.path().canonicalize().unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("repo"), dir.join("link-to-repo")).unwrap();
            std::os::unix::fs::symlink(dir.join("file.txt"), dir.join("link-to-file")).unwrap();
            std::os::unix::fs::symlink(dir.join("missing"), dir.join("dangling")).unwrap();
            let l = list_directory(Some(dir.to_str().unwrap()), false, Some(&dir)).unwrap();
            assert!(names(&l).contains(&"link-to-repo"));
            assert!(!names(&l).contains(&"link-to-file"));
            assert!(!names(&l).contains(&"dangling"));
            let linked = l.entries.iter().find(|e| e.name == "link-to-repo").unwrap();
            assert!(linked.is_git_repo);
            // Listing through the link names the real directory.
            let through = list_directory(
                Some(&dir.join("link-to-repo").to_string_lossy()),
                false,
                Some(&dir),
            )
            .unwrap();
            assert_eq!(through.path, dir.join("repo").to_str().unwrap());
        }
        let dotted = format!("{}/beta/../zeta", dir.display());
        let l = list_directory(Some(&dotted), false, Some(&dir)).unwrap();
        assert_eq!(l.path, dir.join("zeta").to_str().unwrap());
    }

    #[test]
    fn the_root_has_no_parent() {
        let l = list_directory(Some("/"), false, Some(Path::new("/"))).unwrap();
        assert_eq!(l.parent, None);
        assert_eq!(l.path, "/");
    }

    #[test]
    fn failures_say_what_is_wrong() {
        let tmp = tempfile::tempdir().unwrap();
        make(tmp.path());
        let dir = tmp.path().canonicalize().unwrap();
        let missing = list_directory(Some(&format!("{}/nope", dir.display())), false, Some(&dir))
            .unwrap_err();
        assert_eq!(missing.code, ApiErrorCode::NotFound);
        assert!(
            missing.message.contains("does not exist"),
            "{}",
            missing.message
        );

        let file = list_directory(
            Some(&format!("{}/file.txt", dir.display())),
            false,
            Some(&dir),
        )
        .unwrap_err();
        assert_eq!(file.code, ApiErrorCode::InvalidArgument);
        assert!(file.message.contains("not a directory"), "{}", file.message);

        let relative = list_directory(Some("some/where"), false, Some(&dir)).unwrap_err();
        assert_eq!(relative.code, ApiErrorCode::InvalidArgument);
        assert!(
            relative.message.contains("absolute"),
            "{}",
            relative.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_read_is_forbidden() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root can read it anyway (a container); the check is for ordinary users.
        if std::fs::read_dir(&locked).is_err() {
            let e = list_directory(Some(locked.to_str().unwrap()), false, Some(&dir)).unwrap_err();
            assert_eq!(e.code, ApiErrorCode::Forbidden);
            assert!(e.message.contains("permission denied"), "{}", e.message);
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
