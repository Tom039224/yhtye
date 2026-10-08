//! Which harness × model runs each role (Stage 7b, `core-design.md` §15):
//! the settings and their resolution ([`settings`], pure), the harness
//! registry ([`catalog`]) and the model listing read from the harnesses
//! ([`models`]), and which harnesses are installed ([`detect`], [`installed`]).

mod catalog;
mod codex_config;
mod detect;
mod installed;
mod models;
mod openrouter;
mod settings;

pub use catalog::{
    ANTIGRAVITY, AgentCatalog, CLAUDE_CODE, CODEX, DEVIN, GROK_BUILD, HarnessInfo, HarnessPreset,
    MINIMAX_CODE, MODEL_CONFIG_ID, ModelSource, NO_HARNESS_MESSAGE, OPENCODE, Resolved,
};
pub use codex_config::{codex_home, configured_provider, model_provider};
pub use detect::{
    ANTIGRAVITY_COMMAND, ANTIGRAVITY_COMMAND_ALIAS, ANTIGRAVITY_INSTALL_DIRS, ANTIGRAVITY_ROOT_ENV,
    CODEX_COMMAND, DEVIN_COMMAND, GROK_BUILD_COMMAND, GROK_BUILD_INSTALL_DIR, GROK_BUILD_ROOT_ENV,
    HARNESS_SPECS, HarnessDetection, HarnessRequirement, HarnessSpec, KNOWN_DIRS,
    MINIMAX_CODE_COMMAND, MINIMAX_CODE_INSTALL_DIR, MINIMAX_CODE_ROOT_ENV, NPX_COMMAND,
    OPENCODE_COMMAND, PathSource, check_executable_path, detect_harnesses, find_command,
    find_in_path, harness_spec, known_dirs,
};
pub use installed::{
    OPENCODE_FALLBACK_MODEL, default_choice, inherited_opencode_env_remove, presets_from,
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
