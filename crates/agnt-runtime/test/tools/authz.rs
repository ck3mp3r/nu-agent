//! Tool authorization test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

/// Tool-call arguments as rig 0.44 carries them: a JSON object map.
macro_rules! args_map {
    ($($json:tt)*) => {
        serde_json::json!($($json)*)
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
}

// region:    --- Modules

#[path = "authz/ask_grants.rs"]
mod ask_grants;

#[path = "authz/defaults_overlay.rs"]
mod defaults_overlay;

#[path = "authz/display_name.rs"]
mod display_name;

#[path = "authz/grant_cache.rs"]
mod grant_cache;

#[path = "authz/parser.rs"]
mod parser;

#[path = "authz/safe_defaults.rs"]
mod safe_defaults;

#[path = "authz/support.rs"]
mod support;

#[path = "authz/visibility.rs"]
mod visibility;

#[path = "authz/yaml.rs"]
mod yaml;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::tools::authz`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use super::*;
use nu_protocol::{Value, record};
