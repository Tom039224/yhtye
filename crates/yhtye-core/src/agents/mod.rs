//! Which harness × model runs each role (Stage 7b, `core-design.md` §15):
//! the settings and their resolution ([`settings`], pure), the harness
//! registry ([`catalog`]) and the model listing read from the harnesses
//! ([`models`]), and which harnesses are installed ([`installed`]).

mod catalog;
mod installed;
mod models;
mod settings;

pub use catalog::{
    AgentCatalog, CLAUDE_CODE, HarnessInfo, HarnessPreset, MODEL_CONFIG_ID, OPENCODE, Resolved,
};
pub use installed::{
    OPENCODE_COMMAND, OPENCODE_FALLBACK_MODEL, find_in_path, inherited_opencode_env_remove,
    installed_presets,
};
pub use models::{
    EffortOption, HarnessModels, ModelEfforts, ModelOption, ModelService, efforts_from_options,
    models_from_options, probe_efforts, probe_models,
};
pub use settings::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, Candidate, MAX_NOTE_CHARS,
    PickError, RoleSettings, effective,
};
