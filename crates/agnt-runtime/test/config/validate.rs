//! `Config::validate()` tests.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "validate/general.rs"]
mod general;
#[path = "validate/limits.rs"]
mod limits;
#[path = "validate/output_budget.rs"]
mod output_budget;

// endregion: --- Modules

use crate::config::Config;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A minimal config that passes validation on its own.
fn base_config() -> Config {
    Config {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        ..Config::default()
    }
}
