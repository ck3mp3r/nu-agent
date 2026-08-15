use crate::plugin::AgentPlugin;
use nu_agent_core::session::{SessionStore, StoreType};
use nu_agent_core::types::Message;
use nu_plugin::Plugin;

#[test]
fn plugin_has_version() {
    let plugin = AgentPlugin::new();
    let version = plugin.version();
    assert!(!version.is_empty(), "Plugin version should not be empty");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn memory_store_persists_across_create_calls() {
    let plugin = AgentPlugin::new();
    let rt = tokio::runtime::Runtime::new().unwrap();

    let store1 = plugin.create_store_with(StoreType::Memory).unwrap();
    rt.block_on(store1.create("test-session", &[Message::user("hello")]))
        .unwrap();

    let store2 = plugin.create_store_with(StoreType::Memory).unwrap();
    let loaded = rt.block_on(store2.load("test-session")).unwrap();
    assert!(
        loaded.is_some(),
        "session must persist across create_store_with calls"
    );
}
