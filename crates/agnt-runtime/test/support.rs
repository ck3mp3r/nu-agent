//! Test support shared by the runtime's moved modules.

use crate::utils::env_map::EnvMap;

/// Build an [`EnvMap`] from `(key, value)` pairs.
pub(crate) fn env_map(entries: &[(&str, &str)]) -> EnvMap {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}
