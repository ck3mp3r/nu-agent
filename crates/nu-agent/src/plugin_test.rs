use crate::plugin::AgentPlugin;
use nu_agent_core::session::SessionStore;
use nu_plugin::Plugin;
use tempfile::TempDir;

#[test]
fn plugin_has_version() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let plugin = AgentPlugin::new_with_store(store);
    let version = plugin.version();
    assert!(!version.is_empty(), "Plugin version should not be empty");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}
