//! Names of groups and tasks (`orchestration-model.md` §2.1): the orchestrator
//! picks a short slug of the work (`headless-server`), and that name is the id.
//! Older projects have numbered ids (`G-<n>`, `T-<n>`); they stay valid.

use super::types::ToolError;

/// Longest group or task name.
pub const MAX_NAME_CHARS: usize = 32;

/// Names a group cannot take: directories of the worktree root that are not a
/// group's (`<root>/branches/` holds the worktrees of target branches).
const RESERVED_GROUP_NAMES: &[&str] = &["branches"];

/// Whether `name` is a valid group / task name: `^[a-z0-9]+(-[a-z0-9]+)*$`,
/// at most [`MAX_NAME_CHARS`]. So it never contains `--` (which separates the
/// group from the task in a task branch).
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_CHARS
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// `invalid_argument` unless `name` is a valid name for a `what` (`group` / `task`).
pub fn check_name(what: &str, name: &str) -> Result<(), ToolError> {
    if is_valid_name(name) {
        return Ok(());
    }
    Err(ToolError::invalid_argument(format!(
        "invalid {what} name {name:?}: {}",
        name_rule(what)
    )))
}

/// `invalid_argument` unless `name` can name a new group.
pub fn check_group_name(name: &str) -> Result<(), ToolError> {
    check_name("group", name)?;
    if RESERVED_GROUP_NAMES.contains(&name) {
        return Err(ToolError::conflict(format!(
            "the group name {name:?} is reserved by Yhtye; choose another name"
        )));
    }
    Ok(())
}

/// What a valid name looks like (for error messages).
#[must_use]
pub fn name_rule(what: &str) -> String {
    let example = if what == "group" {
        "headless-server"
    } else {
        "remote-icon"
    };
    format!(
        "`name` must be a short English slug of the work in kebab-case: lowercase ASCII \
         letters and digits in words joined by single hyphens (no leading, trailing or double \
         hyphen), at most {MAX_NAME_CHARS} characters, e.g. `{example}`. It becomes the \
         {what} id and must be unique in the project."
    )
}

/// `conflict`: `name` is already used (`where_` says by what).
#[must_use]
pub fn name_taken(what: &str, name: &str, where_: &str) -> ToolError {
    ToolError::conflict(format!(
        "the {what} name {name:?} is already taken ({where_}); choose another name"
    ))
}

/// Whether `id` is a numbered task id of an older Yhtye (`T-<n>`): its branch
/// keeps the old form `yhtye/<group>-T-<n>`.
#[must_use]
pub fn is_numbered_task_id(id: &str) -> bool {
    id.strip_prefix("T-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    #[test]
    fn valid_names_are_kebab_case_slugs_of_at_most_32_chars() {
        for ok in [
            "a",
            "headless-server",
            "remote-icon",
            "v2",
            "a-1-b",
            &"x".repeat(32),
        ] {
            assert!(is_valid_name(ok), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "Headless",
            "G-1",
            "T-3",
            "a_b",
            "a b",
            "a/b",
            "ä",
            "a.b",
            &"x".repeat(33),
        ] {
            assert!(!is_valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn an_invalid_name_states_the_rule_with_an_example() {
        let err = check_name("task", "Remote_Icon").expect_err("invalid");
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        assert!(err.message.contains("kebab-case"), "{}", err.message);
        assert!(err.message.contains("32"), "{}", err.message);
        assert!(err.message.contains("remote-icon"), "{}", err.message);
    }

    #[test]
    fn reserved_group_names_are_refused() {
        let err = check_group_name("branches").expect_err("reserved");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(check_group_name("branch").is_ok());
    }

    #[test]
    fn numbered_task_ids_are_recognised() {
        assert!(is_numbered_task_id("T-1"));
        assert!(is_numbered_task_id("T-99"));
        for other in ["T-", "T-1a", "t-1", "remote-icon", "G-1", "T--1"] {
            assert!(!is_numbered_task_id(other), "{other}");
        }
    }
}
