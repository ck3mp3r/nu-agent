use std::time::Duration;

use tokio::time::timeout;

use super::channel::{CancelTx, PermissionTx};
use super::events::{CancelEvent, PermissionEvent};
use crate::bus::create_bus;
use crate::protocol::event::PermissionRequestContext;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

fn sample_context() -> PermissionRequestContext {
    PermissionRequestContext {
        tool: "read".to_string(),
        source: "user".to_string(),
        mode: None,
        matched_rule_identity: "global:*".to_string(),
        scope: "global".to_string(),
        target_field: None,
        pattern: "*".to_string(),
        summary: "→ {}".to_string(),
        pre_authorize_display: None,
    }
}

// -- PermissionTx::request_permission

#[tokio::test]
async fn request_permission_sends_requested_and_returns_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut permission_rx = bus.permission().subscribe();
    let context = sample_context();

    // -- Exec
    let request_id = bus.permission().request_permission(context.clone()).await?;

    // -- Check
    let event = timeout(Duration::from_millis(100), permission_rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    match event {
        PermissionEvent::Requested {
            request_id: event_id,
            context: event_context,
        } => {
            assert_eq!(
                event_id, request_id,
                "event must carry the returned request_id"
            );
            assert_eq!(
                *event_context, context,
                "event must carry the boxed context"
            );
        }
        other => panic!("expected PermissionEvent::Requested, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn request_permission_returns_unique_ids() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    // Keep a receiver alive so broadcast sends do not fail with NoReceiver.
    let mut _permission_rx = bus.permission().subscribe();
    let context = sample_context();

    // -- Exec
    let id1 = bus.permission().request_permission(context.clone()).await?;
    let id2 = bus.permission().request_permission(context).await?;

    // -- Check
    assert_ne!(id1, id2, "each request must get a unique request_id");
    Ok(())
}

#[tokio::test]
async fn request_permission_fails_when_no_receiver() -> Result<()> {
    // -- Setup & Fixtures
    // A fresh PermissionTx with no subscribers: broadcast send fails with NoReceiver.
    let tx = PermissionTx::new("permission", 64);
    let context = sample_context();

    // -- Exec
    let result = tx.request_permission(context).await;

    // -- Check
    assert!(
        result.is_err(),
        "send with no active receiver must return an error"
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
