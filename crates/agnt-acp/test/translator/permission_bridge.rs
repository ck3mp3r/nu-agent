//! Permission-bridge integration: routing, resolution, and cancellation.

use std::sync::Arc;

use agnt_core::protocol::event::{PermissionDecision, UiEvent};

use super::support::*;
use crate::permission::AcpPermissionBridge;
use crate::translator::{drain_receiver_to_notifications, run_turn_with_notifications};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn test_translator_drain_routes_permission_request_to_bridge() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let permission_sink = RecordingPermissionSink::new(selected("allow-once"));
    let recorder = permission_sink.clone();
    let mut bridge = AcpPermissionBridge::new(permission_sink, SESSION, empty_pending());
    bus.ui_event()
        .request_permission(permission_context())
        .await
        .map_err(|e| format!("should publish: {e:?}"))?;
    drop(bus);

    // -- Exec
    drain_receiver_to_notifications(&mut rx, &sink, &mut translator, &mut bridge).await;

    // -- Check
    let requests = recorder.requests();
    assert_eq!(requests.len(), 1, "the bridge must receive the request");
    assert_eq!(requests[0].session_id, SESSION);
    assert_eq!(requests[0].tool_call.tool_call_id.0.as_ref(), "edit\n{}");
    assert_eq!(requests[0].options.len(), 4);
    assert!(
        sink.taken().is_empty(),
        "a permission request is not a session/update"
    );
    Ok(())
}

#[tokio::test]
async fn test_translator_drain_resolves_pending_permission_with_client_decision() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let pending = empty_pending();
    let mut bridge = permission_bridge(selected("allow-always"), Arc::clone(&pending));
    let request_id = bus
        .ui_event()
        .request_permission(permission_context())
        .await
        .map_err(|e| format!("should publish: {e:?}"))?;
    let decision_rx = register_pending(&pending, &request_id);
    drop(bus);

    // -- Exec
    drain_receiver_to_notifications(&mut rx, &sink, &mut translator, &mut bridge).await;
    let decision = decision_rx.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::AllowAlways);
    Ok(())
}

#[tokio::test]
async fn test_translator_drain_denies_pending_permission_when_bus_closes() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let pending = empty_pending();
    let mut bridge = permission_bridge(selected("allow-once"), Arc::clone(&pending));
    let decision_rx = register_pending(&pending, "perm-never-answered");
    drop(bus);

    // -- Exec
    drain_receiver_to_notifications(&mut rx, &sink, &mut translator, &mut bridge).await;
    let decision = decision_rx.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}

#[tokio::test]
async fn test_translator_drain_denies_pending_permission_when_sink_fails() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::failing();
    let mut translator = translator();
    let pending = empty_pending();
    let mut bridge = permission_bridge(selected("allow-once"), Arc::clone(&pending));
    let decision_rx = register_pending(&pending, "perm-never-answered");
    bus.ui_event()
        .send(UiEvent::AssistantMessage {
            delta: "dropped".to_string(),
            text: "dropped".to_string(),
        })
        .await
        .map_err(|e| format!("should publish: {e:?}"))?;

    // -- Exec
    drain_receiver_to_notifications(&mut rx, &sink, &mut translator, &mut bridge).await;
    let decision = decision_rx.await?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}

#[tokio::test]
async fn test_translator_run_turn_denies_pending_permission_when_turn_completes() -> Result<()> {
    // -- Setup & Fixtures
    let bus = agnt_core::bus::create_bus();
    let mut rx = bus.ui_event().subscribe();
    let sink = RecordingSink::default();
    let mut translator = translator();
    let pending = empty_pending();
    let mut bridge = permission_bridge(selected("allow-once"), Arc::clone(&pending));
    let decision_rx = register_pending(&pending, "perm-never-answered");

    // -- Exec
    let output = run_turn_with_notifications(&mut rx, &sink, &mut translator, &mut bridge, async {
        "done"
    })
    .await;
    let decision = decision_rx.await?;

    // -- Check
    assert_eq!(output, "done");
    assert_eq!(decision, PermissionDecision::Deny);
    Ok(())
}
