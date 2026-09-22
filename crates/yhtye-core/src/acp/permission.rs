//! Automatic answers to `session/request_permission` (pure functions).
//!
//! Options are chosen by `kind`, never by position: `allow_always` first, then
//! `allow_once`. If neither exists the request is answered `Cancelled`.

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome,
};

/// Preference order of the kinds Yhtye accepts.
const PREFERRED_KINDS: [PermissionOptionKind; 2] = [
    PermissionOptionKind::AllowAlways,
    PermissionOptionKind::AllowOnce,
];

/// Picks the option to auto-approve, or `None` when no allow option exists.
#[must_use]
pub fn choose_permission(options: &[PermissionOption]) -> Option<&PermissionOption> {
    PREFERRED_KINDS
        .iter()
        .find_map(|kind| options.iter().find(|o| o.kind == *kind))
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
        choose_permission(options).map(|o| o.option_id.0.to_string())
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
        assert!(choose_permission(&options).is_none());
        assert_eq!(outcome_for(None), RequestPermissionOutcome::Cancelled);
        assert!(choose_permission(&[]).is_none());
    }

    #[test]
    fn outcome_selects_option_id() {
        let options = [opt("allow-once", PermissionOptionKind::AllowOnce)];
        match outcome_for(choose_permission(&options)) {
            RequestPermissionOutcome::Selected(s) => assert_eq!(&*s.option_id.0, "allow-once"),
            other => panic!("unexpected outcome {other:?}"),
        }
    }
}
