//! Which harness × model runs each role (Stage 7b, `core-design.md` §15):
//! the settings and their resolution ([`settings`], pure), the harness
//! registry ([`catalog`]) and the model listing read from the harnesses
//! ([`models`]).

mod catalog;
mod models;
mod settings;

pub use catalog::{AgentCatalog, HarnessInfo, HarnessPreset, MODEL_CONFIG_ID};
pub use models::{HarnessModels, ModelOption, ModelService, models_from_options, probe_models};
pub use settings::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, RoleSettings, effective,
};
