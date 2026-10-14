//! Where the app keeps its data (the database and the worktrees), found the
//! same way for the desktop app and for `--headless` (and for the thin clients
//! of `yhtye remote …`, which must find the same directory): `--data-dir`, else
//! `YHTYE_DATA_DIR`, else the app data directory
//! (`$XDG_DATA_HOME/io.github.tom039224.yhtye`, `~/.local/share/…` without it;
//! the directory Tauri's `app_data_dir` names, which needs no `AppHandle` here).
//! On the default path the pre-rename `com.tom039224.yhtye` directory is moved
//! first ([`crate::legacy_data`]).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::legacy_data;

/// The app identifier (`identifier` of `tauri.conf.json`, which a test checks):
/// the name of the data directory.
pub const APP_IDENTIFIER: &str = "io.github.tom039224.yhtye";

/// Environment variable that names the data directory.
pub const DATA_DIR_ENV: &str = "YHTYE_DATA_DIR";

/// Model of every role unless `--model` / `YHTYE_MODEL` says otherwise (Haiku
/// while the rebuild is being verified; docs/PLAN.md).
pub const DEFAULT_MODEL: &str = "haiku";

/// Environment variable that names the model.
pub const MODEL_ENV: &str = "YHTYE_MODEL";

/// The directory to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    /// Absolute.
    pub path: PathBuf,
    /// It is the app data directory, not one the user named.
    pub is_default: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DataDirError {
    /// No home directory to put the default under.
    NoHome,
    /// The path cannot be made absolute (it is empty or the working directory is gone).
    BadPath(PathBuf, String),
    Create(PathBuf, String),
}

impl fmt::Display for DataDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => write!(
                f,
                "cannot tell where to keep the data (no home directory): set {DATA_DIR_ENV} or pass --data-dir"
            ),
            Self::BadPath(path, e) => write!(f, "bad data directory {}: {e}", path.display()),
            Self::Create(path, e) => write!(f, "cannot create {}: {e}", path.display()),
        }
    }
}

impl std::error::Error for DataDirError {}

impl DataDir {
    /// Finds the directory of this process: `flag` (`--data-dir`), else
    /// `YHTYE_DATA_DIR`, else the app data directory. Creates nothing.
    pub fn resolve(flag: Option<&Path>) -> Result<Self, DataDirError> {
        let env = std::env::var_os(DATA_DIR_ENV).map(PathBuf::from);
        Self::resolve_from(flag, env.as_deref(), dirs::data_dir())
    }

    /// [`DataDir::resolve`] with its inputs given: `data_home` is the base of
    /// the app data directory (`$XDG_DATA_HOME` or `~/.local/share` on Linux).
    /// An empty `flag` / `env` counts as not given.
    pub fn resolve_from(
        flag: Option<&Path>,
        env: Option<&Path>,
        data_home: Option<PathBuf>,
    ) -> Result<Self, DataDirError> {
        let named = [flag, env]
            .into_iter()
            .flatten()
            .find(|p| !p.as_os_str().is_empty());
        let (path, is_default) = match named {
            Some(path) => (path.to_path_buf(), false),
            None => (
                data_home.ok_or(DataDirError::NoHome)?.join(APP_IDENTIFIER),
                true,
            ),
        };
        let path = std::path::absolute(&path)
            .map_err(|e| DataDirError::BadPath(path.clone(), e.to_string()))?;
        Ok(Self { path, is_default })
    }

    /// Moves the legacy directory in on the default path, and creates the
    /// directory.
    pub fn prepare(&self) -> Result<(), DataDirError> {
        if self.is_default {
            legacy_data::migrate(&self.path);
        }
        std::fs::create_dir_all(&self.path)
            .map_err(|e| DataDirError::Create(self.path.clone(), e.to_string()))
    }
}

/// The model: `flag` (`--model`), else `YHTYE_MODEL`, else [`DEFAULT_MODEL`].
#[must_use]
pub fn model(flag: Option<&str>) -> String {
    pick_model(flag, std::env::var(MODEL_ENV).ok().as_deref())
}

fn pick_model(flag: Option<&str>, env: Option<&str>) -> String {
    [flag, env]
        .into_iter()
        .flatten()
        .find(|m| !m.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> Option<PathBuf> {
        Some(PathBuf::from("/home/u/.local/share"))
    }

    #[test]
    fn the_default_is_the_app_data_directory() {
        let dir = DataDir::resolve_from(None, None, home()).expect("dir");
        assert_eq!(
            dir.path,
            Path::new("/home/u/.local/share/io.github.tom039224.yhtye")
        );
        assert!(dir.is_default);
    }

    #[test]
    fn the_flag_beats_the_environment_which_beats_the_default() {
        let flag = Path::new("/srv/a");
        let env = Path::new("/srv/b");
        let by_flag = DataDir::resolve_from(Some(flag), Some(env), home()).expect("dir");
        assert_eq!((by_flag.path.as_path(), by_flag.is_default), (flag, false));
        let by_env = DataDir::resolve_from(None, Some(env), home()).expect("dir");
        assert_eq!((by_env.path.as_path(), by_env.is_default), (env, false));
    }

    #[test]
    fn an_empty_name_counts_as_not_given() {
        let empty = Path::new("");
        let dir = DataDir::resolve_from(Some(empty), Some(empty), home()).expect("dir");
        assert!(dir.is_default);
        let env = Path::new("/srv/b");
        let dir = DataDir::resolve_from(Some(empty), Some(env), home()).expect("dir");
        assert_eq!(dir.path, env);
    }

    #[test]
    fn a_relative_path_is_made_absolute_so_every_process_agrees_on_it() {
        let dir = DataDir::resolve_from(Some(Path::new("data")), None, home()).expect("dir");
        assert!(dir.path.is_absolute());
        assert!(dir.path.ends_with("data"));
    }

    #[test]
    fn without_a_home_the_default_cannot_be_found_but_a_named_directory_can() {
        assert_eq!(
            DataDir::resolve_from(None, None, None),
            Err(DataDirError::NoHome)
        );
        assert!(DataDir::resolve_from(Some(Path::new("/srv/a")), None, None).is_ok());
    }

    #[test]
    fn preparing_a_named_directory_creates_it_and_leaves_its_neighbours_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // A legacy directory next to a *named* one must not be touched.
        let legacy = tmp.path().join(legacy_data::LEGACY_IDENTIFIER);
        std::fs::create_dir(&legacy).expect("legacy");
        let named = tmp.path().join("a").join("b");
        let dir = DataDir::resolve_from(Some(&named), None, None).expect("dir");
        dir.prepare().expect("prepare");
        assert!(named.is_dir());
        assert!(legacy.is_dir());
    }

    #[test]
    fn preparing_the_default_moves_the_legacy_directory_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let legacy = tmp.path().join(legacy_data::LEGACY_IDENTIFIER);
        std::fs::create_dir(&legacy).expect("legacy");
        std::fs::write(legacy.join("yhtye.sqlite3"), b"db").expect("db");
        let dir = DataDir::resolve_from(None, None, Some(tmp.path().to_path_buf())).expect("dir");
        dir.prepare().expect("prepare");
        assert!(!legacy.exists());
        assert_eq!(
            std::fs::read(dir.path.join("yhtye.sqlite3")).expect("moved"),
            b"db"
        );
    }

    #[test]
    fn the_identifier_is_the_one_in_tauri_conf_json() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        assert_eq!(conf["identifier"], APP_IDENTIFIER);
    }

    #[test]
    fn the_model_comes_from_the_flag_then_the_environment_then_haiku() {
        assert_eq!(pick_model(Some("opus"), Some("sonnet")), "opus");
        assert_eq!(pick_model(None, Some("sonnet")), "sonnet");
        assert_eq!(pick_model(Some(""), Some("")), DEFAULT_MODEL);
        assert_eq!(pick_model(None, None), DEFAULT_MODEL);
    }
}
