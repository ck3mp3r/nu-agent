use std::path::{Path, PathBuf};

use crate::config::Paths;
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

/// The application paths of a test rooted at `root`.
///
/// Every artifact sits under `root`, so a test touches only its own tempdir and
/// never the user's config, cache, or data tree.
pub(crate) fn paths_in(root: &Path) -> Paths {
    let logs = root.join("logs");
    Paths {
        config_path: root.join("config.toml"),
        dotenv_path: root.join(".env"),
        agents_dir: root.join("agents"),
        copilot_token_dir: root.join("github_copilot"),
        store_path: root.join("rig-sessions.db"),
        log_dir: logs.clone(),
        log_path: logs.join("agent.log"),
        audit_log_path: root.join("tool_audit.log"),
        secrets_path: root.join("secrets.json"),
        models_cache_path: root.join("models.json"),
        theme_path: root.join("theme.json"),
        grammar_cache_dir: root.join("tree-sitter"),
    }
}
