//! The orchestrator's harness × model arguments (`core-design.md` §15.5):
//! `create_task` overrides are checked against the role's candidates before the
//! state machine sees them, and `get_status` lists the allowed choices.

use serde_json::{Value, json};

use crate::agents::{AgentChoice, AgentRole, AgentSettings, RoleSettings};
use crate::domain::ToolError;
use crate::mcp::tools::CreateTaskArgs;

/// Replaces the agent arguments of `args` with the candidates they select
/// (both harness and model filled in), or rejects them with `invalid_argument`
/// listing the allowed choices.
pub(super) fn choose_agents(
    settings: &AgentSettings,
    mut args: CreateTaskArgs,
) -> Result<CreateTaskArgs, ToolError> {
    let role = AgentRole::of_task(args.kind);
    if let Some(c) = pick(
        settings.get(role),
        role,
        "harness",
        &args.harness,
        &args.model,
    )? {
        args.harness = Some(c.harness);
        args.model = c.model;
    }
    let reviewer = settings.get(AgentRole::Reviewer);
    let review = pick(
        reviewer,
        AgentRole::Reviewer,
        "review_harness",
        &args.review_harness,
        &args.review_model,
    )?;
    if let Some(c) = review {
        args.review_harness = Some(c.harness);
        args.review_model = c.model;
    }
    Ok(args)
}

/// `None` when neither is given (the role default applies at session start).
fn pick(
    settings: &RoleSettings,
    role: AgentRole,
    arg: &str,
    harness: &Option<String>,
    model: &Option<String>,
) -> Result<Option<AgentChoice>, ToolError> {
    if harness.is_none() && model.is_none() {
        return Ok(None);
    }
    settings
        .pick(harness.as_deref(), model.as_deref())
        .map(Some)
        .map_err(|allowed| {
            let asked = [
                harness.as_deref().map(|h| format!("harness={h}")),
                model.as_deref().map(|m| format!("model={m}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            let allowed: Vec<String> = allowed.iter().map(AgentChoice::label).collect();
            ToolError::invalid_argument(format!(
                "{arg}: {asked} is not an allowed {} choice; allowed (harness/model): {} \
                 — or omit it to use the default {}",
                role.as_str(),
                allowed.join(", "),
                settings.default.label(),
            ))
        })
}

/// `get_status`'s `agents`: default and allowed choices per task role.
pub(super) fn agents_status(settings: &AgentSettings) -> Value {
    let role = |r: AgentRole| {
        let s = settings.get(r);
        json!({ "default": s.default, "allowed": s.candidates })
    };
    json!({
        "implementer": role(AgentRole::Implementer),
        "investigator": role(AgentRole::Investigator),
        "reviewer": role(AgentRole::Reviewer),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentSettingsLayer, effective};
    use crate::domain::{ErrorCode, StepKind, StepSpec, TaskKind};

    fn settings() -> AgentSettings {
        let haiku = AgentChoice::new("cc", Some("haiku"));
        let mut layer = AgentSettingsLayer::default();
        layer.set(
            AgentRole::Implementer,
            Some(RoleSettings {
                candidates: vec![haiku.clone(), AgentChoice::new("cc", Some("sonnet"))],
                default: haiku.clone(),
            }),
        );
        layer.set(
            AgentRole::Investigator,
            Some(RoleSettings::only(AgentChoice::new("oc", None))),
        );
        effective(&haiku, &layer, None)
    }

    fn args(kind: TaskKind) -> CreateTaskArgs {
        CreateTaskArgs {
            group_id: "G-1".into(),
            title: "t".into(),
            kind,
            steps: vec![StepSpec {
                kind: StepKind::Implement,
                instruction: None,
            }],
            depends_on: vec![],
            instruction: None,
            harness: None,
            model: None,
            review_harness: None,
            review_model: None,
        }
    }

    #[test]
    fn omitted_arguments_stay_omitted() {
        let a = args(TaskKind::Code);
        assert_eq!(choose_agents(&settings(), a.clone()), Ok(a));
    }

    #[test]
    fn an_allowed_choice_is_filled_in() {
        let mut a = args(TaskKind::Code);
        a.model = Some("sonnet".into());
        a.review_harness = Some("cc".into());
        let out = choose_agents(&settings(), a).expect("allowed");
        assert_eq!(
            (out.harness.as_deref(), out.model.as_deref()),
            (Some("cc"), Some("sonnet"))
        );
        assert_eq!(
            (out.review_harness.as_deref(), out.review_model.as_deref()),
            (Some("cc"), Some("haiku")),
            "the reviewer role's builtin candidate"
        );
    }

    #[test]
    fn investigate_tasks_use_the_investigator_candidates() {
        let mut a = args(TaskKind::Investigate);
        a.harness = Some("oc".into());
        let out = choose_agents(&settings(), a).expect("allowed");
        assert_eq!((out.harness.as_deref(), out.model), (Some("oc"), None));
        let mut a = args(TaskKind::Investigate);
        a.model = Some("sonnet".into());
        let e = choose_agents(&settings(), a).expect_err("not an investigator choice");
        assert_eq!(e.code, ErrorCode::InvalidArgument);
        assert!(
            e.message.contains("allowed (harness/model): oc"),
            "{}",
            e.message
        );
    }

    #[test]
    fn a_choice_outside_the_candidates_is_rejected_with_the_allowed_list() {
        let mut a = args(TaskKind::Code);
        a.harness = Some("cc".into());
        a.model = Some("opus".into());
        let e = choose_agents(&settings(), a).expect_err("outside");
        assert_eq!(e.code, ErrorCode::InvalidArgument);
        assert!(
            e.message
                .starts_with("harness: harness=cc model=opus is not an allowed implementer choice"),
            "{}",
            e.message
        );
        assert!(e.message.contains("cc/haiku, cc/sonnet"), "{}", e.message);
        let mut a = args(TaskKind::Code);
        a.review_model = Some("sonnet".into());
        let e = choose_agents(&settings(), a).expect_err("reviewer");
        assert!(
            e.message
                .starts_with("review_harness: model=sonnet is not an allowed reviewer choice"),
            "{}",
            e.message
        );
    }

    #[test]
    fn status_lists_defaults_and_candidates() {
        let v = agents_status(&settings());
        assert_eq!(
            v["implementer"]["default"],
            json!({"harness": "cc", "model": "haiku"})
        );
        assert_eq!(
            v["implementer"]["allowed"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(
            v["investigator"]["default"],
            json!({"harness": "oc", "model": null})
        );
        assert!(v.get("orchestrator").is_none());
    }
}
