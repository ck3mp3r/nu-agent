use crate::plugin::AgentPlugin;
use nu_agent_core::session::{SessionStore, StoreType};
use nu_agent_core::types::Message;
use nu_plugin::Plugin;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn plugin_has_version() {
    let plugin = AgentPlugin::default();
    let version = plugin.version();
    assert!(!version.is_empty(), "Plugin version should not be empty");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn memory_store_persists_across_create_calls() -> Result<()> {
    let plugin = AgentPlugin::default();
    let rt = tokio::runtime::Runtime::new()?;

    let store1 = plugin.create_store_with(StoreType::Memory)?;
    rt.block_on(store1.create("test-session", &[Message::user("hello")]))?;

    let store2 = plugin.create_store_with(StoreType::Memory)?;
    let loaded = rt.block_on(store2.load("test-session"))?;
    assert!(
        loaded.is_some(),
        "session must persist across create_store_with calls"
    );
    Ok(())
}
