use super::AgentPlugin;
use crate::llm::runtime::LlmRuntime;
use crate::session::SessionStore;
use nu_plugin::Plugin;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn plugin_has_version() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let plugin = AgentPlugin::new_with_store(store, Arc::new(LlmRuntime::new()));
    let version = plugin.version();
    assert!(!version.is_empty(), "Plugin version should not be empty");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn plugin_owns_single_runtime_instance() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let rt = Arc::new(LlmRuntime::new());
    let plugin = AgentPlugin::new_with_store(store, rt.clone());

    let a = plugin.llm_runtime();
    let b = plugin.llm_runtime();
    let c = plugin.runtime_ctx();
    let d = plugin.runtime_ctx();

    assert!(Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(&a, &rt));
    assert_eq!(c.llm_runtime() as *const _, d.llm_runtime() as *const _);
    assert_eq!(c.llm_runtime() as *const _, a.as_ref() as *const _);
}
