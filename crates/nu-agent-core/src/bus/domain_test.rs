use std::time::Duration;

use tokio::time::timeout;

use super::channel::CancelTx;
use super::events::CancelEvent;
use crate::bus::create_bus;
use crate::protocol::event::{PermissionRequestContext, UiEvent};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- UiEventTx::request_permission

#[tokio::test]
async fn ui_event_request_permission_sends_requested_and_returns_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();
    let context = PermissionRequestContext {
        tool: "edit".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: None,
    };

    // -- Exec
    let request_id = bus.ui_event().request_permission(context.clone()).await?;

    // -- Check
    assert!(
        request_id.starts_with("perm-"),
        "returned request_id must start with 'perm-'"
    );
    let event = timeout(Duration::from_millis(100), ui_event_rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    match event {
        UiEvent::PermissionRequested {
            request_id: event_id,
            context: event_context,
        } => {
            assert_eq!(
                event_id, request_id,
                "event must carry the returned request_id"
            );
            assert_eq!(
                event_context, context,
                "event must carry the unboxed context"
            );
        }
        other => panic!("expected UiEvent::PermissionRequested, got {other:?}"),
    }
    Ok(())
}

// -- UiEventTx::request_permission no-receiver error path

#[tokio::test]
async fn ui_event_request_permission_fails_when_no_receiver() -> Result<()> {
    // -- Setup & Fixtures
    // A fresh UiEventTx with no subscribers: the broadcast send fails with
    // NoReceiver. `InteractivePermissionResolver::resolve` relies on this
    // error to fall back to a generated request ID
    // (`unwrap_or_else(|_| next_request_id())`).
    let tx = super::channel::UiEventTx::new("ui_event", 64);
    let context = PermissionRequestContext {
        tool: "edit".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        matched_rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        pre_authorize_display: None,
    };

    // -- Exec
    let result = tx.request_permission(context).await;

    // -- Check
    assert_eq!(
        result,
        Err(super::channel::ChannelError::NoReceiver),
        "send with no active receiver must return NoReceiver"
    );
    Ok(())
}

// -- CancelTx::request_cancel

#[tokio::test]
async fn request_cancel_sends_requested() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut cancel_rx = bus.cancel().subscribe();

    // -- Exec
    bus.cancel().request_cancel().await?;

    // -- Check
    let event = timeout(Duration::from_millis(100), cancel_rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(event, CancelEvent::Requested));
    Ok(())
}

#[tokio::test]
async fn request_cancel_fails_when_no_receiver() -> Result<()> {
    // -- Setup & Fixtures
    let tx = CancelTx::new("cancel", 64);

    // -- Exec
    let result = tx.request_cancel().await;

    // -- Check
    assert!(
        result.is_err(),
        "send with no active receiver must return an error"
    );
    Ok(())
}
