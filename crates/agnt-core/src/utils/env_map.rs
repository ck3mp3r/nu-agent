//! Environment map for dependency-injected environment lookups.
//!
//! Production code reads the real process environment through [`process_env`].
//! Tests build an explicit map, so they never mutate process-global state and
//! can run in parallel.

// region:    --- Modules

use std::collections::HashMap;

// endregion: --- Modules

// region:    --- Types

/// A snapshot of environment variables keyed by name.
pub type EnvMap = HashMap<String, String>;

// endregion: --- Types

// region:    --- Public Functions

/// Snapshot the current process environment.
///
/// Non-Unicode entries are skipped: `std::env::vars()` panics on them, and a
/// config lookup can never match a non-Unicode name anyway.
pub fn process_env() -> EnvMap {
    std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

// endregion: --- Public Functions
