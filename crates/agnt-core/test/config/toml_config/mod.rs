//! `config.toml` loading tests.
//!
//! The suite is split into topical sub-files. Shared imports and fixtures live
//! here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

mod cmd_refs;
mod discovery;
mod hooks;
mod load;
mod paths;
mod pricing;
mod support;
mod theme;
mod vault_refs;

// endregion: --- Modules

use crate::compaction::CompactionStrategy;
use crate::config::toml_config::{TomlConfigError, load_from, load_from_with};
use crate::config::{AgentsConfig, PluginConfig};

use support::*;
