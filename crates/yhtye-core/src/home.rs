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
