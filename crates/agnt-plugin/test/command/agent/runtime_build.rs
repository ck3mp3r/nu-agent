//! Unit tests for `runtime_build` config resolution.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "runtime_build/cli_flags.rs"]
mod cli_flags;

#[path = "runtime_build/persona_config.rs"]
mod persona_config;

#[path = "runtime_build/persona_model.rs"]
mod persona_model;

#[path = "runtime_build/preamble.rs"]
mod preamble;

#[path = "runtime_build/support.rs"]
mod support;

// The original monolith resolved `super::<fn>` against the production
// `runtime_build` module. Sub-files resolve `super::` against this module
// root, so the production functions are re-exported here.
use support::*;

// endregion: --- Modules
