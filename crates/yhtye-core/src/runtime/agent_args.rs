//! The orchestrator's harness × model × effort arguments (`core-design.md`
//! §15.5): `create_task` overrides are checked against the role's candidate rows
//! before the state machine sees them, and `get_status` lists the allowed rows.

use serde_json::{Value, json};

use crate::agents::{AgentChoice, AgentRole, AgentSettings, Candidate, PickError, RoleSettings};
use crate::domain::ToolError;
use crate::mcp::tools::CreateTaskArgs;

/// The argument names of one role's override (`harness`… or `review_harness`…).
struct ArgNames {
    harness: &'static str,
    effort: &'static str,
}

/// Replaces the agent arguments of `args` with the candidate row they select
/// (harness, model and effort filled in), or rejects them with
/// `invalid_argument` listing the candidate rows.
pub(super) fn choose_agents(
    settings: &AgentSettings,
    mut args: CreateTaskArgs,
) -> Result<CreateTaskArgs, ToolError> {
    let role = AgentRole::of_task(args.kind);
    let names = ArgNames {
        harness: "harness",
        effort: "effort",
    };
    let asked = (&args.harness, &args.model, &args.effort);
    if let Some(c) = pick(settings.get(role), role, &names, asked)? {
        args.harness = Some(c.harness);
        args.model = c.model;
        args.effort = c.effort;
    }
    let names = ArgNames {
        harness: "review_harness",
        effort: "review_effort",
    };
    let asked = (
        &args.review_harness,
        &args.review_model,
        &args.review_effort,
    );
    let reviewer = settings.get(AgentRole::Reviewer);
    if let Some(c) = pick(reviewer, AgentRole::Reviewer, &names, asked)? {
        args.review_harness = Some(c.harness);
        args.review_model = c.model;
        args.review_effort = c.effort;
    }
    Ok(args)
}

type Asked<'a> = (&'a Option<String>, &'a Option<String>, &'a Option<String>);

/// `None` when nothing is given (the role default applies at session start).
fn pick(
    settings: &RoleSettings,
    role: AgentRole,
    names: &ArgNames,
    (harness, model, effort): Asked<'_>,
) -> Result<Option<AgentChoice>, ToolError> {
    if harness.is_none() && model.is_none() && effort.is_none() {
        return Ok(None);
    }
    let rows = |rows: &[Candidate]| {
        rows.iter()
            .map(Candidate::describe)
            .collect::<Vec<_>>()
            .join("; ")
    };
    settings
        .pick(harness.as_deref(), model.as_deref(), effort.as_deref())
        .map(|c| Some(c.choice()))
        .map_err(|e| match e {
            PickError::NoMatch(all) => {
                let asked = [
                    harness.as_deref().map(|h| format!("harness={h}")),
                    model.as_deref().map(|m| format!("model={m}")),
                    effort.as_deref().map(|e| format!("effort={e}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                ToolError::invalid_argument(format!(
                    "{}: {asked} is not an allowed {} choice — harness × model × effort must \
                     exactly match one candidate row; candidate rows: {} \
                     — or omit the arguments to use the default {}",
                    names.harness,
                    role.as_str(),
                    rows(&all),
                    Candidate::from(settings.default.clone()).describe(),
                ))
            }
            PickError::NeedsEffort(matching) => ToolError::invalid_argument(format!(
                "{}: several {} candidate rows match; also give `{}` to choose one of: {}",
                names.harness,
                role.as_str(),
                names.effort,
                rows(&matching),
            )),
        })
}

/// `get_status`'s `agents`: default and candidate rows (with notes) per task role.
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
                candidates: vec![
                    haiku.clone().into(),
                    Candidate::from(AgentChoice::new("cc", Some("sonnet")).with_effort("low"))
                        .with_note("quick edits"),
                    Candidate::from(AgentChoice::new("cc", Some("sonnet")).with_effort("high"))
                        .with_note("hard bugs"),
                ],
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
            effort: None,
            review_effort: None,
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
        a.model = Some("haiku".into());
        a.review_harness = Some("cc".into());
        let out = choose_agents(&settings(), a).expect("allowed");
        assert_eq!(
            (out.harness.as_deref(), out.model.as_deref(), out.effort),
            (Some("cc"), Some("haiku"), None)
        );
        assert_eq!(
            (out.review_harness.as_deref(), out.review_model.as_deref()),
            (Some("cc"), Some("haiku")),
            "the reviewer role's builtin candidate"
        );
    }

    #[test]
    fn an_exact_triple_is_accepted_and_a_missing_effort_needs_a_unique_row() {
        let mut a = args(TaskKind::Code);
        a.harness = Some("cc".into());
        a.model = Some("sonnet".into());
        a.effort = Some("high".into());
        let out = choose_agents(&settings(), a).expect("exact row");
        assert_eq!(out.effort.as_deref(), Some("high"));

        // Two rows for cc/sonnet: the effort must be named.
        let mut a = args(TaskKind::Code);
        a.harness = Some("cc".into());
        a.model = Some("sonnet".into());
        let e = choose_agents(&settings(), a).expect_err("ambiguous");
        assert_eq!(e.code, ErrorCode::InvalidArgument);
        assert!(e.message.contains("also give `effort`"), "{}", e.message);
        assert!(
            e.message.contains(
                "effort=low (quick edits); harness=cc model=sonnet effort=high (hard bugs)"
            ),
            "{}",
            e.message
        );

        // An effort no row has is not silently dropped.
        let mut a = args(TaskKind::Code);
        a.harness = Some("cc".into());
        a.model = Some("sonnet".into());
        a.effort = Some("max".into());
        let e = choose_agents(&settings(), a).expect_err("no such row");
        assert!(
            e.message.contains("effort=max is not an allowed"),
            "{}",
            e.message
        );
        assert!(
            e.message.contains("harness=cc model=haiku;"),
            "{}",
            e.message
        );

        // The reviewer arguments are named review_*.
        let mut a = args(TaskKind::Code);
        a.review_effort = Some("high".into());
        let e = choose_agents(&settings(), a).expect_err("reviewer has no efforts");
        assert!(
            e.message.starts_with("review_harness: effort=high"),
            "{}",
            e.message
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
            e.message.contains("candidate rows: harness=oc"),
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
        assert!(
            e.message
                .contains("harness=cc model=haiku; harness=cc model=sonnet effort=low"),
            "{}",
            e.message
        );
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
            json!({"harness": "cc", "model": "haiku", "effort": null})
        );
        assert_eq!(
            v["implementer"]["allowed"].as_array().map(Vec::len),
            Some(3)
        );
        assert_eq!(v["implementer"]["allowed"][2]["note"], "hard bugs");
        assert_eq!(
            v["investigator"]["default"],
            json!({"harness": "oc", "model": null, "effort": null})
        );
        assert!(v.get("orchestrator").is_none());
    }
}
