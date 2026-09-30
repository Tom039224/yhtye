//! Automatic answers to `session/request_permission` (pure functions).
//!
//! Options are chosen by `kind`, never by position. By default `allow_always`
//! comes first, then `allow_once`; [`PermissionPolicy::OnceOnly`] takes only a
//! plain `allow_once`. If nothing fits the request is answered `Cancelled`.

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome,
};

use super::config::PermissionPolicy;

/// Preference order of the kinds Yhtye accepts.
const PREFERRED_KINDS: [PermissionOptionKind; 2] = [
    PermissionOptionKind::AllowAlways,
    PermissionOptionKind::AllowOnce,
];

/// Option id prefixes that change how the session runs (mode / plan switches).
const SESSION_CHANGING_PREFIXES: [&str; 2] = ["switch_", "plan_"];

/// Suffix of the options that grant a standing permission.
const ALWAYS_SUFFIX: &str = "_always";

/// Picks the option to auto-approve under `policy`, or `None` when no
/// acceptable option exists.
#[must_use]
pub fn choose_permission(
    options: &[PermissionOption],
    policy: PermissionPolicy,
) -> Option<&PermissionOption> {
    match policy {
        PermissionPolicy::Default => PREFERRED_KINDS
            .iter()
            .find_map(|kind| options.iter().find(|o| o.kind == *kind)),
        PermissionPolicy::OnceOnly => options
            .iter()
            .find(|o| o.kind == PermissionOptionKind::AllowOnce && !changes_session_or_persists(o)),
    }
}

/// Whether the id of `option` names a mode / plan switch or a standing
/// permission, whatever its `kind` says.
fn changes_session_or_persists(option: &PermissionOption) -> bool {
    let id = &*option.option_id.0;
    SESSION_CHANGING_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
        || id.ends_with(ALWAYS_SUFFIX)
}

/// The protocol outcome for a choice made by [`choose_permission`].
#[must_use]
pub fn outcome_for(choice: Option<&PermissionOption>) -> RequestPermissionOutcome {
    match choice {
        Some(o) => {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(o.option_id.clone()))
        }
        None => RequestPermissionOutcome::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &str, kind: PermissionOptionKind) -> PermissionOption {
        PermissionOption::new(id.to_string(), id.to_string(), kind)
    }

    fn chosen_id(options: &[PermissionOption]) -> Option<String> {
        choose_permission(options, PermissionPolicy::Default).map(|o| o.option_id.0.to_string())
    }

    fn once_only_id(options: &[PermissionOption]) -> Option<String> {
        choose_permission(options, PermissionPolicy::OnceOnly).map(|o| o.option_id.0.to_string())
    }

    #[test]
    fn prefers_allow_always_over_allow_once_regardless_of_order() {
        let options = [
            opt("reject", PermissionOptionKind::RejectOnce),
            opt("once", PermissionOptionKind::AllowOnce),
            opt("always", PermissionOptionKind::AllowAlways),
        ];
        assert_eq!(chosen_id(&options).as_deref(), Some("always"));
    }

    #[test]
    fn falls_back_to_allow_once_and_never_picks_first_reject() {
        let options = [
            opt("reject-always", PermissionOptionKind::RejectAlways),
            opt("reject", PermissionOptionKind::RejectOnce),
            opt("once", PermissionOptionKind::AllowOnce),
        ];
        assert_eq!(chosen_id(&options).as_deref(), Some("once"));
    }

    #[test]
    fn cancels_when_no_allow_option() {
        let options = [
            opt("reject", PermissionOptionKind::RejectOnce),
            opt("reject-always", PermissionOptionKind::RejectAlways),
        ];
        assert!(choose_permission(&options, PermissionPolicy::Default).is_none());
        assert_eq!(outcome_for(None), RequestPermissionOutcome::Cancelled);
        assert!(choose_permission(&[], PermissionPolicy::Default).is_none());
    }

    #[test]
    fn outcome_selects_option_id() {
        let options = [opt("allow-once", PermissionOptionKind::AllowOnce)];
        match outcome_for(choose_permission(&options, PermissionPolicy::Default)) {
            RequestPermissionOutcome::Selected(s) => assert_eq!(&*s.option_id.0, "allow-once"),
            other => panic!("unexpected outcome {other:?}"),
        }
    }

    #[test]
    fn once_only_takes_allow_once_even_when_allow_always_is_offered() {
        let options = [
            opt("allow_always", PermissionOptionKind::AllowAlways),
            opt("allow", PermissionOptionKind::AllowOnce),
            opt("reject", PermissionOptionKind::RejectOnce),
        ];
        assert_eq!(once_only_id(&options).as_deref(), Some("allow"));
        assert_eq!(
            chosen_id(&options).as_deref(),
            Some("allow_always"),
            "the default policy is unchanged"
        );
    }

    #[test]
    fn once_only_skips_mode_switch_plan_and_always_options() {
        let options = [
            opt("switch_to_accept_edits", PermissionOptionKind::AllowOnce),
            opt("plan_approve", PermissionOptionKind::AllowOnce),
            opt("allow_command_always", PermissionOptionKind::AllowOnce),
            opt("allow_always", PermissionOptionKind::AllowOnce),
            opt("allow_once", PermissionOptionKind::AllowOnce),
        ];
        assert_eq!(once_only_id(&options).as_deref(), Some("allow_once"));
    }

    #[test]
    fn once_only_cancels_when_only_switching_or_standing_options_remain() {
        let options = [
            opt("switch_to_bypass", PermissionOptionKind::AllowOnce),
            opt("plan_reject", PermissionOptionKind::AllowOnce),
            opt("allow_always", PermissionOptionKind::AllowAlways),
            opt("reject", PermissionOptionKind::RejectOnce),
        ];
        assert!(choose_permission(&options, PermissionPolicy::OnceOnly).is_none());
        assert!(choose_permission(&[], PermissionPolicy::OnceOnly).is_none());
        assert_eq!(
            outcome_for(choose_permission(&options, PermissionPolicy::OnceOnly)),
            RequestPermissionOutcome::Cancelled
        );
    }
}
