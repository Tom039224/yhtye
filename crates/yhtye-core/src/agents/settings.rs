//! Agent settings: which harness × model each role may use (its candidates)
//! and which one it uses by default, layered global → project
//! (`core-design.md` §15.1). Everything here is pure.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::{Role, TaskKind};

/// A role as far as agent selection is concerned. `Investigator` is the agent
/// of an `investigate` task (a domain `Role::Implementer` session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Orchestrator,
    Implementer,
    Investigator,
    Reviewer,
}

impl AgentRole {
    /// The selection role of a session of `role` working on a task of `kind`.
    #[must_use]
    pub fn of_session(role: Role, kind: Option<TaskKind>) -> Self {
        match (role, kind) {
            (Role::Orchestrator, _) => Self::Orchestrator,
            (Role::Reviewer, _) => Self::Reviewer,
            (Role::Implementer, Some(TaskKind::Investigate)) => Self::Investigator,
            (Role::Implementer, _) => Self::Implementer,
        }
    }

    /// The role whose candidates the agent of a task of `kind` comes from.
    #[must_use]
    pub fn of_task(kind: TaskKind) -> Self {
        Self::of_session(Role::Implementer, Some(kind))
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Implementer => "implementer",
            Self::Investigator => "investigator",
            Self::Reviewer => "reviewer",
        }
    }
}

/// One harness × model combination. `model: None` is the harness's own default
/// (for harnesses that expose no model option).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct AgentChoice {
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
}

impl AgentChoice {
    #[must_use]
    pub fn new(harness: impl Into<String>, model: Option<&str>) -> Self {
        Self {
            harness: harness.into(),
            model: model.map(str::to_string),
        }
    }

    /// `harness/model`, or just `harness` for its default model.
    #[must_use]
    pub fn label(&self) -> String {
        match &self.model {
            Some(m) => format!("{}/{m}", self.harness),
            None => self.harness.clone(),
        }
    }
}

/// The candidates of one role and its default (which is one of them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoleSettings {
    pub candidates: Vec<AgentChoice>,
    pub default: AgentChoice,
}

impl RoleSettings {
    /// Only `choice`.
    #[must_use]
    pub fn only(choice: AgentChoice) -> Self {
        Self {
            candidates: vec![choice.clone()],
            default: choice,
        }
    }

    /// Checks the settings against the registered harnesses; returns them with
    /// duplicate candidates removed.
    pub fn validate(&self, harnesses: &[&str]) -> Result<Self, String> {
        let mut candidates: Vec<AgentChoice> = Vec::new();
        for c in &self.candidates {
            check_choice(c, harnesses)?;
            if !candidates.contains(c) {
                candidates.push(c.clone());
            }
        }
        if candidates.is_empty() {
            return Err("at least one candidate is required".into());
        }
        if !candidates.contains(&self.default) {
            return Err(format!(
                "the default {} is not one of the candidates",
                self.default.label()
            ));
        }
        Ok(Self {
            candidates,
            default: self.default.clone(),
        })
    }

    /// The candidate matching `harness` / `model` (either may be omitted): the
    /// default when both are omitted or it matches, else the first match.
    /// `Err` lists the candidates when nothing matches.
    pub fn pick(
        &self,
        harness: Option<&str>,
        model: Option<&str>,
    ) -> Result<AgentChoice, Vec<AgentChoice>> {
        let matches = |c: &AgentChoice| {
            harness.is_none_or(|h| c.harness == h)
                && model.is_none_or(|m| c.model.as_deref() == Some(m))
        };
        if matches(&self.default) {
            return Ok(self.default.clone());
        }
        self.candidates
            .iter()
            .find(|c| matches(c))
            .cloned()
            .ok_or_else(|| self.candidates.clone())
    }
}

fn check_choice(c: &AgentChoice, harnesses: &[&str]) -> Result<(), String> {
    if c.harness.trim().is_empty() {
        return Err("a candidate has an empty harness".into());
    }
    if c.model.as_deref().is_some_and(|m| m.trim().is_empty()) {
        return Err(format!("{}: the model is empty", c.harness));
    }
    if !harnesses.contains(&c.harness.as_str()) {
        return Err(format!(
            "unknown harness {} (known: {})",
            c.harness,
            harnesses.join(", ")
        ));
    }
    Ok(())
}

/// The settings in effect for every role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct AgentSettings {
    pub orchestrator: RoleSettings,
    pub implementer: RoleSettings,
    pub investigator: RoleSettings,
    pub reviewer: RoleSettings,
}

impl AgentSettings {
    #[must_use]
    pub fn get(&self, role: AgentRole) -> &RoleSettings {
        match role {
            AgentRole::Orchestrator => &self.orchestrator,
            AgentRole::Implementer => &self.implementer,
            AgentRole::Investigator => &self.investigator,
            AgentRole::Reviewer => &self.reviewer,
        }
    }
}

/// One layer (global or a project): a role without settings inherits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
pub struct AgentSettingsLayer {
    pub orchestrator: Option<RoleSettings>,
    pub implementer: Option<RoleSettings>,
    pub investigator: Option<RoleSettings>,
    pub reviewer: Option<RoleSettings>,
}

impl AgentSettingsLayer {
    #[must_use]
    pub fn get(&self, role: AgentRole) -> Option<&RoleSettings> {
        self.slot(role).as_ref()
    }

    /// Sets (or with `None` clears) the settings of `role`.
    pub fn set(&mut self, role: AgentRole, settings: Option<RoleSettings>) {
        *self.slot_mut(role) = settings;
    }

    fn slot(&self, role: AgentRole) -> &Option<RoleSettings> {
        match role {
            AgentRole::Orchestrator => &self.orchestrator,
            AgentRole::Implementer => &self.implementer,
            AgentRole::Investigator => &self.investigator,
            AgentRole::Reviewer => &self.reviewer,
        }
    }

    fn slot_mut(&mut self, role: AgentRole) -> &mut Option<RoleSettings> {
        match role {
            AgentRole::Orchestrator => &mut self.orchestrator,
            AgentRole::Implementer => &mut self.implementer,
            AgentRole::Investigator => &mut self.investigator,
            AgentRole::Reviewer => &mut self.reviewer,
        }
    }
}

/// The settings in effect: per role, the project's, else the global ones,
/// else only `builtin`.
#[must_use]
pub fn effective(
    builtin: &AgentChoice,
    global: &AgentSettingsLayer,
    project: Option<&AgentSettingsLayer>,
) -> AgentSettings {
    let role = |r: AgentRole| {
        project
            .and_then(|p| p.get(r))
            .or_else(|| global.get(r))
            .cloned()
            .unwrap_or_else(|| RoleSettings::only(builtin.clone()))
    };
    AgentSettings {
        orchestrator: role(AgentRole::Orchestrator),
        implementer: role(AgentRole::Implementer),
        investigator: role(AgentRole::Investigator),
        reviewer: role(AgentRole::Reviewer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(h: &str, m: Option<&str>) -> AgentChoice {
        AgentChoice::new(h, m)
    }

    fn settings(candidates: &[AgentChoice], default: &AgentChoice) -> RoleSettings {
        RoleSettings {
            candidates: candidates.to_vec(),
            default: default.clone(),
        }
    }

    #[test]
    fn sessions_map_to_selection_roles() {
        use AgentRole as A;
        assert_eq!(A::of_session(Role::Orchestrator, None), A::Orchestrator);
        assert_eq!(
            A::of_session(Role::Reviewer, Some(TaskKind::Code)),
            A::Reviewer
        );
        assert_eq!(
            A::of_session(Role::Implementer, Some(TaskKind::Code)),
            A::Implementer
        );
        assert_eq!(A::of_task(TaskKind::Investigate), A::Investigator);
    }

    #[test]
    fn layers_override_per_role_and_fall_back_to_the_builtin() {
        let builtin = c("cc", Some("haiku"));
        let mut global = AgentSettingsLayer::default();
        let g = settings(
            &[c("cc", Some("haiku")), c("cc", Some("sonnet"))],
            &c("cc", Some("sonnet")),
        );
        global.set(AgentRole::Implementer, Some(g.clone()));
        global.set(AgentRole::Reviewer, Some(g.clone()));
        let mut project = AgentSettingsLayer::default();
        let p = RoleSettings::only(c("oc", None));
        project.set(AgentRole::Reviewer, Some(p.clone()));

        let e = effective(&builtin, &global, Some(&project));
        assert_eq!(e.orchestrator, RoleSettings::only(builtin.clone()));
        assert_eq!(e.implementer, g);
        assert_eq!(e.reviewer, p, "the project layer wins");
        assert_eq!(e.investigator, RoleSettings::only(builtin.clone()));
        let e = effective(&builtin, &global, None);
        assert_eq!(
            e.reviewer, g,
            "without the project the global layer is used"
        );

        project.set(AgentRole::Reviewer, None);
        assert_eq!(effective(&builtin, &global, Some(&project)).reviewer, g);
    }

    #[test]
    fn validation_requires_known_harnesses_and_a_default_among_the_candidates() {
        let known = ["cc", "oc"];
        let ok = settings(
            &[
                c("cc", Some("haiku")),
                c("cc", Some("haiku")),
                c("oc", None),
            ],
            &c("oc", None),
        );
        let v = ok.validate(&known).expect("valid");
        assert_eq!(v.candidates.len(), 2, "duplicates are removed");

        let empty = settings(&[], &c("cc", None));
        assert!(empty.validate(&known).unwrap_err().contains("at least one"));
        let outside = settings(&[c("cc", Some("haiku"))], &c("cc", Some("opus")));
        assert!(
            outside
                .validate(&known)
                .unwrap_err()
                .contains("not one of the candidates")
        );
        let unknown = settings(&[c("xx", None)], &c("xx", None));
        assert!(
            unknown
                .validate(&known)
                .unwrap_err()
                .contains("unknown harness xx")
        );
        let blank = settings(&[c("cc", Some(" "))], &c("cc", Some(" ")));
        assert!(
            blank
                .validate(&known)
                .unwrap_err()
                .contains("model is empty")
        );
    }

    #[test]
    fn pick_prefers_the_default_then_the_first_match() {
        let s = settings(
            &[
                c("cc", Some("haiku")),
                c("cc", Some("sonnet")),
                c("oc", Some("a")),
                c("oc", Some("b")),
            ],
            &c("cc", Some("sonnet")),
        );
        assert_eq!(s.pick(None, None), Ok(c("cc", Some("sonnet"))));
        assert_eq!(s.pick(Some("cc"), None), Ok(c("cc", Some("sonnet"))));
        assert_eq!(s.pick(Some("oc"), None), Ok(c("oc", Some("a"))));
        assert_eq!(s.pick(None, Some("b")), Ok(c("oc", Some("b"))));
        assert_eq!(
            s.pick(Some("cc"), Some("haiku")),
            Ok(c("cc", Some("haiku")))
        );
        assert_eq!(s.pick(Some("cc"), Some("opus")), Err(s.candidates.clone()));
        assert_eq!(s.pick(Some("zz"), None), Err(s.candidates.clone()));
    }

    #[test]
    fn a_model_less_candidate_only_matches_without_a_model() {
        let s = RoleSettings::only(c("oc", None));
        assert_eq!(s.pick(Some("oc"), None), Ok(c("oc", None)));
        assert!(s.pick(Some("oc"), Some("x")).is_err());
    }

    #[test]
    fn labels_show_the_model_when_there_is_one() {
        assert_eq!(c("cc", Some("haiku")).label(), "cc/haiku");
        assert_eq!(c("oc", None).label(), "oc");
    }
}
