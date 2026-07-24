use crate::plugin::AgentPlugin;
use nu_plugin::Plugin;

#[test]
fn plugin_has_version() {
    let plugin = AgentPlugin::new();
    let version = plugin.version();
    assert!(!version.is_empty(), "Plugin version should not be empty");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
}
