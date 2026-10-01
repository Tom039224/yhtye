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

/// One harness × model × effort combination. `model: None` is the harness's
/// own default (for harnesses that expose no model option); `effort: None` means
/// "do not set the effort option" (Stage 7d, `core-design.md` §15.1). Old stored
/// values without `effort` read as `None`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct AgentChoice {
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

impl AgentChoice {
    #[must_use]
    pub fn new(harness: impl Into<String>, model: Option<&str>) -> Self {
        Self {
            harness: harness.into(),
            model: model.map(str::to_string),
            effort: None,
        }
    }

    #[must_use]
    pub fn with_effort(mut self, effort: &str) -> Self {
        self.effort = Some(effort.to_string());
        self
    }

    /// `harness/model effort=high`; just `harness` for its default model, no
    /// `effort=` part when the effort is not set.
    #[must_use]
    pub fn label(&self) -> String {
        let mut text = match &self.model {
            Some(m) => format!("{}/{m}", self.harness),
            None => self.harness.clone(),
        };
        if let Some(e) = &self.effort {
            text.push_str(&format!(" effort={e}"));
        }
        text
    }
}

/// Longest allowed [`Candidate::note`] (characters).
pub const MAX_NOTE_CHARS: usize = 400;

/// A candidate row: an [`AgentChoice`] plus a free-text `note` saying when to
/// use it (shown to the orchestrator, who picks by it). The same
/// harness × model may appear in several rows with different efforts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Candidate {
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub note: String,
}

impl Candidate {
    /// The row's combination without the note.
    #[must_use]
    pub fn choice(&self) -> AgentChoice {
        AgentChoice {
            harness: self.harness.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
        }
    }

    #[must_use]
    pub fn with_note(mut self, note: &str) -> Self {
        self.note = note.to_string();
        self
    }

    /// `harness=cc model=sonnet effort=high (note)`: the arguments the
    /// orchestrator passes (a model may contain `/`, so no `harness/model`).
    #[must_use]
    pub fn describe(&self) -> String {
        let mut text = format!("harness={}", self.harness);
        match &self.model {
            Some(m) => text.push_str(&format!(" model={m}")),
            None => text.push_str(" (its default model)"),
        }
        if let Some(e) = &self.effort {
            text.push_str(&format!(" effort={e}"));
        }
        if !self.note.trim().is_empty() {
            text.push_str(&format!(" ({})", self.note.trim()));
        }
        text
    }
}

impl From<AgentChoice> for Candidate {
    fn from(c: AgentChoice) -> Self {
        Self {
            harness: c.harness,
            model: c.model,
            effort: c.effort,
            note: String::new(),
        }
    }
}

/// Why [`RoleSettings::pick`] found no single candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickError {
    /// Nothing matches; carries all candidates.
    NoMatch(Vec<Candidate>),
    /// The rows for the asked harness × model differ only in effort and none was
    /// named; carries those rows.
    NeedsEffort(Vec<Candidate>),
}

/// The candidates of one role and its default (which is one of them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoleSettings {
    pub candidates: Vec<Candidate>,
    pub default: AgentChoice,
}

impl RoleSettings {
    /// Only `choice`.
    #[must_use]
    pub fn only(choice: AgentChoice) -> Self {
        Self {
            candidates: vec![choice.clone().into()],
            default: choice,
        }
    }

    /// Checks the settings against the registered harnesses; returns them with
    /// duplicate rows (same harness × model × effort, the first one wins) removed.
    pub fn validate(&self, harnesses: &[&str]) -> Result<Self, String> {
        self.validate_keeping(harnesses, &[])
    }

    /// [`RoleSettings::validate`], except that a row of a harness that is not
    /// registered passes when `kept` has a row with the same harness × model ×
    /// effort: the settings stored earlier keep the rows of a harness that was
    /// uninstalled since (to bring them back), and editing the others must not
    /// fail because of them. A new row of an unknown harness is still refused.
    pub fn validate_keeping(&self, harnesses: &[&str], kept: &[Candidate]) -> Result<Self, String> {
        let mut candidates: Vec<Candidate> = Vec::new();
        for c in &self.candidates {
            check_candidate(c)?;
            if !kept.iter().any(|k| k.choice() == c.choice()) {
                check_registered(c, harnesses)?;
            }
            if candidates.iter().all(|x| x.choice() != c.choice()) {
                candidates.push(c.clone());
            }
        }
        if candidates.is_empty() {
            return Err("at least one candidate is required".into());
        }
        if candidates.iter().all(|c| c.choice() != self.default) {
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

    /// The candidate row selected by `harness` / `model` / `effort` (each may
    /// be omitted; an omitted one matches anything). Nothing omitted: an exact
    /// match. When several rows match: the default if it is one of them, unless
    /// the matches are all the same harness × model (with `effort` omitted) —
    /// then the caller must name the effort ([`PickError::NeedsEffort`]).
    pub fn pick(
        &self,
        harness: Option<&str>,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<Candidate, PickError> {
        let matches: Vec<&Candidate> = self
            .candidates
            .iter()
            .filter(|c| {
                harness.is_none_or(|h| c.harness == h)
                    && model.is_none_or(|m| c.model.as_deref() == Some(m))
                    && effort.is_none_or(|e| c.effort.as_deref() == Some(e))
            })
            .collect();
        let Some(first) = matches.first() else {
            return Err(PickError::NoMatch(self.candidates.clone()));
        };
        if matches.len() == 1 {
            return Ok((*first).clone());
        }
        let same_model = matches
            .iter()
            .all(|c| c.harness == first.harness && c.model == first.model);
        if same_model && effort.is_none() {
            return Err(PickError::NeedsEffort(
                matches.into_iter().cloned().collect(),
            ));
        }
        let chosen = matches
            .iter()
            .find(|c| c.choice() == self.default)
            .unwrap_or(first);
        Ok((*chosen).clone())
    }
}

fn check_candidate(c: &Candidate) -> Result<(), String> {
    if c.harness.trim().is_empty() {
        return Err("a candidate has an empty harness".into());
    }
    if c.model.as_deref().is_some_and(|m| m.trim().is_empty()) {
        return Err(format!("{}: the model is empty", c.harness));
    }
    if c.effort.as_deref().is_some_and(|e| e.trim().is_empty()) {
        return Err(format!("{}: the effort is empty", c.harness));
    }
    if c.note.chars().count() > MAX_NOTE_CHARS {
        return Err(format!(
            "{}: the note is longer than {MAX_NOTE_CHARS} characters",
            c.harness
        ));
    }
    Ok(())
}

fn check_registered(c: &Candidate, harnesses: &[&str]) -> Result<(), String> {
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
            candidates: candidates.iter().cloned().map(Candidate::from).collect(),
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
    fn validation_lets_the_rows_already_stored_keep_an_unknown_harness() {
        let known = ["cc"];
        let gone = c("oc", Some("x"));
        let stored = settings(
            &[gone.clone(), c("cc", Some("haiku"))],
            &c("cc", Some("haiku")),
        );
        // Kept as they are, and edited around: the row of `oc` passes, a note
        // on it is not a new harness either.
        let edited = RoleSettings {
            candidates: vec![
                Candidate::from(gone.clone()).with_note("cheap"),
                c("cc", Some("haiku")).into(),
                c("cc", Some("sonnet")).into(),
            ],
            default: c("cc", Some("sonnet")),
        };
        assert!(edited.validate(&known).is_err(), "plain validation refuses");
        let v = edited
            .validate_keeping(&known, &stored.candidates)
            .expect("the stored row stays");
        assert_eq!(v.candidates.len(), 3);

        // Another unknown harness, or the same harness with another model, is new.
        for added in [c("zz", None), c("oc", Some("y"))] {
            let mut more = edited.clone();
            more.candidates.push(added.clone().into());
            let e = more
                .validate_keeping(&known, &stored.candidates)
                .expect_err("a new row of an unknown harness");
            assert!(
                e.contains(&format!("unknown harness {}", added.harness)),
                "{e}"
            );
        }
    }

    fn cand(h: &str, m: &str, e: Option<&str>) -> Candidate {
        Candidate {
            harness: h.into(),
            model: Some(m.into()),
            effort: e.map(str::to_string),
            note: String::new(),
        }
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
        let all: Vec<Candidate> = s.candidates.clone();
        assert_eq!(
            s.pick(None, None, None).map(|x| x.choice()),
            Ok(c("cc", Some("sonnet")))
        );
        assert_eq!(
            s.pick(Some("cc"), None, None).map(|x| x.choice()),
            Ok(c("cc", Some("sonnet")))
        );
        assert_eq!(
            s.pick(Some("oc"), None, None).map(|x| x.choice()),
            Ok(c("oc", Some("a")))
        );
        assert_eq!(
            s.pick(None, Some("b"), None).map(|x| x.choice()),
            Ok(c("oc", Some("b")))
        );
        assert_eq!(
            s.pick(Some("cc"), Some("haiku"), None).map(|x| x.choice()),
            Ok(c("cc", Some("haiku")))
        );
        assert_eq!(
            s.pick(Some("cc"), Some("opus"), None),
            Err(PickError::NoMatch(all.clone()))
        );
        assert_eq!(s.pick(Some("zz"), None, None), Err(PickError::NoMatch(all)));
    }

    #[test]
    fn the_same_model_may_appear_with_different_efforts() {
        let rows = vec![
            cand("cc", "sonnet", Some("low")).with_note("quick fixes"),
            cand("cc", "sonnet", Some("high")).with_note("hard bugs"),
            cand("cc", "haiku", None),
        ];
        let s = RoleSettings {
            candidates: rows.clone(),
            default: rows[0].choice(),
        };
        assert!(s.validate(&["cc"]).is_ok(), "same model, different efforts");

        // An exact triple selects its row, with its note.
        let hit = s
            .pick(Some("cc"), Some("sonnet"), Some("high"))
            .expect("row");
        assert_eq!(
            (hit.effort.as_deref(), hit.note.as_str()),
            (Some("high"), "hard bugs")
        );
        // An effort no row has does not match, even for a known model.
        assert_eq!(
            s.pick(Some("cc"), Some("sonnet"), Some("max")),
            Err(PickError::NoMatch(rows.clone()))
        );
        // The effort of a row without one cannot be named.
        assert!(matches!(
            s.pick(Some("cc"), Some("haiku"), Some("low")),
            Err(PickError::NoMatch(_))
        ));
        // Several rows for the model and no effort: the caller must choose,
        // even though one of them is the default.
        assert_eq!(
            s.pick(Some("cc"), Some("sonnet"), None),
            Err(PickError::NeedsEffort(rows[..2].to_vec()))
        );
        // Exactly one row for the model: an omitted effort takes that row's.
        assert_eq!(
            s.pick(Some("cc"), Some("haiku"), None).map(|x| x.effort),
            Ok(None)
        );
        let only = RoleSettings {
            candidates: vec![cand("cc", "opus", Some("max"))],
            default: cand("cc", "opus", Some("max")).choice(),
        };
        assert_eq!(
            only.pick(Some("cc"), Some("opus"), None).map(|x| x.effort),
            Ok(Some("max".into()))
        );
    }

    #[test]
    fn duplicate_rows_are_removed_by_their_triple_and_the_default_must_match_exactly() {
        let rows = vec![
            cand("cc", "sonnet", Some("low")).with_note("first"),
            cand("cc", "sonnet", Some("low")).with_note("second"),
            cand("cc", "sonnet", None),
        ];
        let s = RoleSettings {
            candidates: rows.clone(),
            default: rows[0].choice(),
        };
        let v = s.validate(&["cc"]).expect("valid");
        assert_eq!(v.candidates.len(), 2);
        assert_eq!(v.candidates[0].note, "first");
        let bad = RoleSettings {
            candidates: rows,
            default: cand("cc", "sonnet", Some("high")).choice(),
        };
        assert!(
            bad.validate(&["cc"])
                .unwrap_err()
                .contains("not one of the candidates")
        );
    }

    #[test]
    fn empty_efforts_and_long_notes_are_rejected() {
        let mut row = cand("cc", "sonnet", Some(" "));
        let s = |c: &Candidate| RoleSettings {
            candidates: vec![c.clone()],
            default: c.choice(),
        };
        assert!(
            s(&row)
                .validate(&["cc"])
                .unwrap_err()
                .contains("effort is empty")
        );
        row.effort = None;
        row.note = "x".repeat(MAX_NOTE_CHARS + 1);
        assert!(
            s(&row)
                .validate(&["cc"])
                .unwrap_err()
                .contains("note is longer")
        );
        row.note = "x".repeat(MAX_NOTE_CHARS);
        assert!(s(&row).validate(&["cc"]).is_ok());
    }

    #[test]
    fn settings_stored_before_stage_7d_read_with_no_effort_and_an_empty_note() {
        let old = r#"{"candidates":[{"harness":"cc","model":"haiku"},{"harness":"oc"}],
                      "default":{"harness":"cc","model":"haiku"}}"#;
        let s: RoleSettings = serde_json::from_str(old).expect("old JSON");
        assert_eq!(s.candidates[0].effort, None);
        assert_eq!(s.candidates[0].note, "");
        assert_eq!(s.candidates[1].model, None);
        assert_eq!(s.default, c("cc", Some("haiku")));
        assert!(s.validate(&["cc", "oc"]).is_ok());
        let task_agent: AgentChoice =
            serde_json::from_str(r#"{"harness":"cc","model":null}"#).expect("old task agent");
        assert_eq!(task_agent.effort, None);
    }

    #[test]
    fn a_model_less_candidate_only_matches_without_a_model() {
        let s = RoleSettings::only(c("oc", None));
        assert_eq!(
            s.pick(Some("oc"), None, None).map(|x| x.choice()),
            Ok(c("oc", None))
        );
        assert!(s.pick(Some("oc"), Some("x"), None).is_err());
    }

    #[test]
    fn labels_show_the_model_when_there_is_one() {
        assert_eq!(c("cc", Some("haiku")).label(), "cc/haiku");
        assert_eq!(c("oc", None).label(), "oc");
        assert_eq!(
            c("cc", Some("sonnet")).with_effort("high").label(),
            "cc/sonnet effort=high"
        );
    }
}
