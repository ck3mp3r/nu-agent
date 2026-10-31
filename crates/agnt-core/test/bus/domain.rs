use std::time::Duration;

use tokio::time::timeout;

use super::channel::CancelTx;
use super::events::CancelEvent;
use crate::bus::create_bus;
use crate::protocol::event::PermissionRequestContext;
use crate::protocol::events::SystemEvent;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

fn permission_context() -> PermissionRequestContext {
    PermissionRequestContext {
        tool: "edit".to_string(),
        tool_key: "edit\n{}".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: None,
    }
}

// -- UiEventTx::request_permission

/// `request_permission` publishes a `PermissionRequested` event carrying the
/// returned request id; with no receiver it fails with `NoReceiver`.
#[tokio::test]
async fn ui_event_request_permission() -> Result<()> {
    // -- Case 1: a subscriber receives the event with the returned id
    let bus = create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();
    let context = permission_context();

    let request_id = bus.ui_event().request_permission(context.clone()).await?;

    assert!(
        request_id.as_str().starts_with("perm-"),
        "returned request_id must start with 'perm-'"
    );
    let event = timeout(Duration::from_millis(100), ui_event_rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    match event {
        SystemEvent::PermissionRequested {
            request_id: event_id,
            context: event_context,
        } => {
            assert_eq!(
                event_id.as_str(),
                request_id.as_str(),
                "event must carry the returned request_id"
            );
            assert_eq!(
                event_context, context,
                "event must carry the unboxed context"
            );
        }
        other => panic!("expected SystemEvent::PermissionRequested, got {other:?}"),
    }

    // -- Case 2: no receiver fails with NoReceiver
    // A fresh UiEventTx with no subscribers: the broadcast send fails with
    // NoReceiver. `InteractivePermissionResolver::resolve` relies on this
    // error to fall back to a generated request ID
    // (`unwrap_or_else(|_| next_request_id())`).
    let tx = super::channel::UiEventTx::new("ui_event", 64);
    let result = tx.request_permission(permission_context()).await;
    assert_eq!(
        result,
        Err(super::channel::ChannelError::NoReceiver),
        "send with no active receiver must return NoReceiver"
    );
    Ok(())
}

// -- CancelTx::request_cancel

/// `request_cancel` publishes a `Requested` cancel event; with no receiver it
/// returns an error.
#[tokio::test]
async fn request_cancel() -> Result<()> {
    // -- Case 1: a subscriber receives the cancel request
    let bus = create_bus();
    let mut cancel_rx = bus.cancel().subscribe();
    bus.cancel().request_cancel().await?;
    let event = timeout(Duration::from_millis(100), cancel_rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(event, CancelEvent::Requested));

    // -- Case 2: no receiver returns an error
    let tx = CancelTx::new("cancel", 64);
    assert!(
        tx.request_cancel().await.is_err(),
        "send with no active receiver must return an error"
    );
    Ok(())
}
