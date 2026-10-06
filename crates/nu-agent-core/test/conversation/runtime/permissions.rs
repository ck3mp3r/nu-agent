use super::*;

// ========================================================================
// Permission state tests
// ========================================================================

#[test]
fn permissions_startup_summary_does_not_emit_warning() {
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let bus = crate::bus::create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();
    let summary =
        "permissions policy: overlay_active=false global=ask tool_rules=5 nested_command_rules=1";

    let mut state = crate::conversation::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        PermissionsConfig::safe_defaults(true),
        None,
        SessionGrantCache::default(),
        summary.to_string(),
    );

    state.emit_startup_summary_once();
    state.emit_startup_summary_once();

    let mut count = 0usize;
    loop {
        match ui_event_rx.try_recv() {
            Ok(crate::protocol::event::UiEvent::Warning { .. }) => count += 1,
            Ok(_) => {}
            Err(crate::bus::TryRecvError::Empty) => break,
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(crate::bus::TryRecvError::Closed) => break,
        }
    }
    assert_eq!(
        count, 0,
        "policy summary goes to the log, not the warning channel"
    );
}

#[test]
fn set_permissions_replaces_config_and_does_not_emit_warning() -> Result<()> {
    use crate::tools::authz::{PermissionAction, PermissionsConfig, SessionGrantCache};

    let initial = PermissionsConfig::safe_defaults(true);
    let summary = format!(
        "permissions policy: overlay_active=false global={} tool_rules={}",
        initial.summary().global.as_str(),
        initial.summary().tool_rule_count
    );

    let mut state = crate::conversation::state::permission::PermissionState::new(
        initial.clone(),
        initial,
        None,
        SessionGrantCache::default(),
        "initial".to_string(),
    );

    // Verify initial state before set_permissions
    assert_eq!(state.permissions().summary().global, PermissionAction::Ask);

    // Create a new config with different global action via overlay
    use crate::tools::authz::PermissionsOverlay;
    let mut deny_mapping = noyalib::Mapping::new();
    deny_mapping.insert("*", noyalib::Value::String("deny".to_string()));
    let deny_overlay = PermissionsOverlay::parse_from_yaml(&deny_mapping)
        .map_err(|e| format!("valid overlay: {e:?}"))?;
    let new_permissions = state.permissions().with_overlay(&deny_overlay);

    state.set_permissions(new_permissions, summary.clone());

    // Config is replaced
    assert_eq!(state.permissions().summary().global, PermissionAction::Deny);

    let bus = crate::bus::create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();
    state.emit_startup_summary_once();
    state.emit_startup_summary_once();

    let mut count = 0usize;
    loop {
        match ui_event_rx.try_recv() {
            Ok(crate::protocol::event::UiEvent::Warning { .. }) => count += 1,
            Ok(_) => {}
            Err(crate::bus::TryRecvError::Empty) => break,
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(crate::bus::TryRecvError::Closed) => break,
        }
    }
    assert_eq!(
        count, 0,
        "policy summary goes to the log, not the warning channel"
    );
    Ok(())
}
