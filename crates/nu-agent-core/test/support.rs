use std::path::PathBuf;

use crate::utils::env_map::EnvMap;

pub(crate) fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/fixtures/markdown")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read markdown fixture {}: {error}",
            path.display()
        )
    })
}

/// Build an [`EnvMap`] from `(key, value)` pairs.
pub(crate) fn env_map(entries: &[(&str, &str)]) -> EnvMap {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}
