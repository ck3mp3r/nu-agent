//! Session-class hook spawning: the zero-hook cases spawn nothing.

use super::support::*;
use super::*;

#[test]
fn test_mode_spawn_configured_hooks_without_a_config_spawns_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_runtime::bus::Bus::default();
    let process = RecordingProcess::default();

    // -- Exec
    let handles = spawn_configured_hooks(
        None,
        agnt_runtime::hooks::HookSources {
            turn: bus.turn(),
            ui_event: bus.ui_event(),
            compaction_tx: bus.compaction(),
            session: bus.session(),
        },
        std::path::Path::new("/tmp"),
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
    let bus = agnt_runtime::bus::Bus::default();
    let process = RecordingProcess::default();
    let config = PluginConfig::default();

    // -- Exec
    let handles = spawn_configured_hooks(
        Some(&config),
        agnt_runtime::hooks::HookSources {
            turn: bus.turn(),
            ui_event: bus.ui_event(),
            compaction_tx: bus.compaction(),
            session: bus.session(),
        },
        std::path::Path::new("/tmp"),
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
