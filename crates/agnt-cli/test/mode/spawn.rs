//! Session-class hook spawning: the zero-hook cases spawn nothing.

use super::support::*;
use super::*;

#[test]
fn test_mode_spawn_configured_hooks_without_a_config_spawns_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let process = RecordingProcess::default();

    // -- Exec
    let handles = spawn_configured_hooks(
        None,
        &bus,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    );

    // -- Check
    assert!(
        handles.is_empty(),
        "no plugin config must spawn no hook runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}

#[test]
fn test_mode_spawn_configured_hooks_with_empty_hooks_spawns_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let process = RecordingProcess::default();
    let config = PluginConfig::default();

    // -- Exec
    let handles = spawn_configured_hooks(
        Some(&config),
        &bus,
        std::path::Path::new("/tmp"),
        &agent_context(),
        process.clone(),
    );

    // -- Check
    assert!(
        handles.is_empty(),
        "an empty hooks config must spawn no hook runner"
    );
    assert!(process.calls().is_empty(), "no hook process may be invoked");
    Ok(())
}
