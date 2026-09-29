//! Which harness × model runs each role (Stage 7b, `core-design.md` §15):
//! the settings and their resolution ([`settings`], pure), the harness
//! registry ([`catalog`]) and the model listing read from the harnesses
//! ([`models`]), and which harnesses are installed ([`installed`]).

mod catalog;
mod codex_config;
mod installed;
mod models;
mod openrouter;
mod settings;

pub use catalog::{
    AgentCatalog, CLAUDE_CODE, CODEX, HarnessInfo, HarnessPreset, MODEL_CONFIG_ID, ModelSource,
    OPENCODE, Resolved,
};
pub use codex_config::{codex_home, configured_provider, model_provider};
pub use installed::{
    CODEX_COMMAND, OPENCODE_COMMAND, OPENCODE_FALLBACK_MODEL, find_in_path,
    inherited_opencode_env_remove, installed_presets,
};
pub use models::{
    EffortOption, HarnessModels, ModelEfforts, ModelOption, ModelService, efforts_from_options,
    models_from_options, probe_efforts, probe_models,
};
pub use openrouter::{
    OPENROUTER_MODELS_URL, OPENROUTER_PROVIDER, fetch_openrouter_models, models_from_openrouter,
};
pub use settings::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, Candidate, MAX_NOTE_CHARS,
    PickError, RoleSettings, effective,
};
