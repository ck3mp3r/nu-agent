//! `PluginConfig::resolve_model()` core resolution tests.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "resolve_model/basics.rs"]
mod basics;
#[path = "resolve_model/cache.rs"]
mod cache;
#[path = "resolve_model/env_fallback.rs"]
mod env_fallback;
#[path = "resolve_model/errors.rs"]
mod errors;
#[path = "resolve_model/provider.rs"]
mod provider;

// endregion: --- Modules

use std::collections::HashMap;

use super::support::plugin_config_with_cache;
use crate::config::{
    AgentsConfig, ModelConfig, ModelLimits, ModelRoleConfig, PluginConfig, ProviderConfig,
};
use crate::utils::env_map::EnvMap;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
