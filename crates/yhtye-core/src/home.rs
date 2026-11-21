//! The home directory of the user, read from an injected environment so it can
//! be tested (`~` in the harnesses' install and configuration paths).

use std::ffi::OsString;
use std::path::PathBuf;

/// The user's home directory in `env`: `HOME` on Unix; `USERPROFILE`, else
/// `HOMEDRIVE` + `HOMEPATH`, on Windows. An empty value counts as unset. The
/// result is not checked to be absolute or to exist.
pub fn home_dir_in(env: &(impl Fn(&str) -> Option<OsString> + ?Sized)) -> Option<PathBuf> {
    home_dir_for(cfg!(windows), env)
}

/// [`home_dir_in`] for either platform (so both are testable anywhere).
pub(crate) fn home_dir_for(
    windows: bool,
    env: &(impl Fn(&str) -> Option<OsString> + ?Sized),
) -> Option<PathBuf> {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty());
    if !windows {
        return non_empty("HOME").map(PathBuf::from);
    }
    non_empty("USERPROFILE").map(PathBuf::from).or_else(|| {
        let mut home = non_empty("HOMEDRIVE")?;
        home.push(non_empty("HOMEPATH")?);
        Some(PathBuf::from(home))
    })
}

/// What tests that feed an injected environment need to run on either OS.
#[cfg(test)]
pub(crate) mod test_support {
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// The variable [`home_dir_in`](super::home_dir_in) reads on this OS.
    pub(crate) const HOME: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

    /// `path` as an absolute path of this OS: a rooted Unix-style literal (`/h/x`) gets a drive
    /// on Windows (`C:/h/x`), where it would otherwise be relative.
    pub(crate) fn abs(path: &str) -> PathBuf {
        if cfg!(windows) && path.starts_with('/') {
            PathBuf::from(format!("C:{path}"))
        } else {
            PathBuf::from(path)
        }
    }

    /// A lookup over `vars` in which the key `HOME` stands for [`HOME`] and values are made
    /// absolute with [`abs`] (an empty value stays empty).
    pub(crate) fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: std::collections::HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| {
                let key = if *k == "HOME" { HOME } else { k };
                (key.to_string(), abs(v).into_os_string())
            })
            .collect();
        move |k| map.get(k).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn unix_reads_home_only() {
        let vars = [("HOME", "/home/u"), ("USERPROFILE", "C:\\Users\\u")];
        assert_eq!(
            home_dir_for(false, &env(&vars)),
            Some(PathBuf::from("/home/u"))
        );
        assert_eq!(
            home_dir_for(false, &env(&[("USERPROFILE", "C:\\Users\\u")])),
            None
        );
        assert_eq!(home_dir_for(false, &env(&[("HOME", "")])), None);
    }

    #[test]
    fn windows_prefers_userprofile_then_homedrive_and_homepath() {
        let all = [
            ("HOME", "/git/home"),
            ("USERPROFILE", "C:\\Users\\u"),
            ("HOMEDRIVE", "D:"),
            ("HOMEPATH", "\\Users\\v"),
        ];
        assert_eq!(
            home_dir_for(true, &env(&all)),
            Some(PathBuf::from("C:\\Users\\u"))
        );
        assert_eq!(
            home_dir_for(true, &env(&all[2..])),
            Some(PathBuf::from("D:\\Users\\v"))
        );
        let empty_profile = [
            ("USERPROFILE", ""),
            ("HOMEDRIVE", "D:"),
            ("HOMEPATH", "\\h"),
        ];
        assert_eq!(
            home_dir_for(true, &env(&empty_profile)),
            Some(PathBuf::from("D:\\h")),
            "an empty USERPROFILE counts as unset"
        );
        assert_eq!(home_dir_for(true, &env(&[("HOMEDRIVE", "D:")])), None);
        assert_eq!(home_dir_for(true, &env(&[("HOME", "/git/home")])), None);
    }
}
