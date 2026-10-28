//! Whether a host's Yhtye and this app can work together (a port of
//! `mobile/…/version/Compatibility.kt`; the host tells its version in the answer
//! to `ping`).

use std::fmt;

use serde::Serialize;

/// `major.minor.patch`, and the pre-release identifier if there is one
/// (`0.2.0-beta.1`). Build metadata (`+…`) is read and dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre_release: Option<String>,
}

impl AppVersion {
    /// `0.1.0` or `1.2.3-beta.1+build` (surrounding spaces are fine); `None`
    /// when it is not a Semantic Version. No leading `v`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (core_and_pre, build) = match text.split_once('+') {
            Some((head, build)) => (head, Some(build)),
            None => (text, None),
        };
        if build.is_some_and(|b| !valid_identifiers(b)) {
            return None;
        }
        let (core, pre) = match core_and_pre.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (core_and_pre, None),
        };
        let mut numbers = core.split('.');
        let major = number(numbers.next()?)?;
        let minor = number(numbers.next()?)?;
        let patch = number(numbers.next()?)?;
        if numbers.next().is_some() {
            return None;
        }
        if pre.is_some_and(|p| !valid_pre_release(p)) {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre_release: pre.map(str::to_string),
        })
    }
}

impl fmt::Display for AppVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre_release {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

/// A number without a leading zero (`0` itself is fine).
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn valid_identifiers(text: &str) -> bool {
    text.split('.')
        .all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// Dot-separated identifiers; a numeric one has no leading zero.
fn valid_pre_release(text: &str) -> bool {
    valid_identifiers(text)
        && text
            .split('.')
            .all(|id| !id.bytes().all(|b| b.is_ascii_digit()) || id == "0" || !id.starts_with('0'))
}

/// How a host's version and this app's version fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Compatibility {
    Compatible,
    /// They do not fit: one of them needs an update.
    Incompatible {
        host: String,
        app: String,
    },
    /// The host's version is not known (a Yhtye that predates the field, or an
    /// unreadable one). No warning is due.
    Unknown,
}

/// Compatibility of a host's version (`host`: what its `ping` answered; none,
/// empty or unreadable is [`Compatibility::Unknown`]) with this app's.
///
/// - While the major version is 0, the minor version must be the same (0.x
///   may break at any minor version).
/// - From 1.0 on: the same major version, and the host's minor version at
///   least the app's (a newer host has only added to the API).
/// - The patch version and the pre-release identifier are not looked at.
#[must_use]
pub fn compatibility(host: Option<&str>, app: &AppVersion) -> Compatibility {
    let Some(host_version) = host.and_then(AppVersion::parse) else {
        return Compatibility::Unknown;
    };
    let compatible = if host_version.major != app.major {
        false
    } else if app.major == 0 {
        host_version.minor == app.minor
    } else {
        host_version.minor >= app.minor
    };
    if compatible {
        Compatibility::Compatible
    } else {
        Compatibility::Incompatible {
            host: host_version.to_string(),
            app: app.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> AppVersion {
        AppVersion::parse(text).expect(text)
    }

    #[test]
    fn versions_are_read_with_pre_release_and_build_parts() {
        assert_eq!(
            v("0.1.0"),
            AppVersion {
                major: 0,
                minor: 1,
                patch: 0,
                pre_release: None
            }
        );
        assert_eq!(v(" 1.2.3-beta.1+build.5 ").to_string(), "1.2.3-beta.1");
        for bad in [
            "", "1", "1.2", "1.2.3.4", "v1.2.3", "01.2.3", "1.2.3-", "1.2.3-01", "1.2.3+", "a.b.c",
            "1.2.-3",
        ] {
            assert_eq!(AppVersion::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn while_major_is_zero_the_minor_must_match() {
        let app = v("0.1.0");
        assert_eq!(
            compatibility(Some("0.1.9"), &app),
            Compatibility::Compatible
        );
        assert_eq!(
            compatibility(Some("0.1.0-beta.1"), &app),
            Compatibility::Compatible
        );
        assert_eq!(
            compatibility(Some("0.2.0"), &app),
            Compatibility::Incompatible {
                host: "0.2.0".into(),
                app: "0.1.0".into()
            }
        );
        assert!(matches!(
            compatibility(Some("0.0.9"), &app),
            Compatibility::Incompatible { .. }
        ));
        assert!(matches!(
            compatibility(Some("1.0.0"), &app),
            Compatibility::Incompatible { .. }
        ));
    }

    #[test]
    fn from_one_zero_a_newer_minor_host_still_works() {
        let app = v("1.2.0");
        assert_eq!(
            compatibility(Some("1.2.5"), &app),
            Compatibility::Compatible
        );
        assert_eq!(
            compatibility(Some("1.9.0"), &app),
            Compatibility::Compatible
        );
        assert!(matches!(
            compatibility(Some("1.1.9"), &app),
            Compatibility::Incompatible { .. }
        ));
        assert!(matches!(
            compatibility(Some("2.0.0"), &app),
            Compatibility::Incompatible { .. }
        ));
        assert!(matches!(
            compatibility(Some("0.2.0"), &app),
            Compatibility::Incompatible { .. }
        ));
    }

    #[test]
    fn an_unknown_host_version_is_not_a_conflict() {
        let app = v("0.1.0");
        for host in [None, Some(""), Some("   "), Some("not a version")] {
            assert_eq!(
                compatibility(host, &app),
                Compatibility::Unknown,
                "{host:?}"
            );
        }
    }
}
