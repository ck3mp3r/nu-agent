use std::time::Duration;

use tokio::time::timeout;

use crate::bus::{Bus, PermissionCard, RequestCancel, request_permission, settle_permission};
use crate::protocol::event::PermissionRequestContext;
use agnt_kernel::bus::{CancelEvent, CancelTx, ChannelError, FactRecord, PermissionGate};
use agnt_kernel::{PermissionRecord, SessionRecord};

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
    }
}

// -- Bus::request_permission

/// The gate snapshot and the stream record are the same record value, and the
/// returned id is the record's id.
#[tokio::test]
async fn request_permission_mints_one_record_for_both_concerns() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut facts_rx = bus.facts().subscribe();
    let mut stream_rx = bus.stream().subscribe();
    let card = PermissionCard {
        shows_preview: true,
        content: Some("--- a/x\n+++ b/x\n".to_string()),
    };

    // -- Exec
    let request_id = request_permission(bus.stream(), bus.facts(), permission_context(), card)?;

    // -- Check
    assert!(
        request_id.as_str().starts_with("perm-"),
        "returned request_id must start with 'perm-'"
    );
    let snapshot = timeout(Duration::from_millis(100), facts_rx.recv())
        .await
        .map_err(|_| "facts receive should not time out")?
        .map_err(|_| "facts receive should succeed")?;
    let record = timeout(Duration::from_millis(100), stream_rx.recv())
        .await
        .map_err(|_| "stream receive should not time out")?
        .map_err(|_| "stream receive should succeed")?;
    let FactRecord::PermissionGate(PermissionGate::Prompting(gate)) = snapshot else {
        return Err("facts must carry the prompting snapshot".into());
    };
    let SessionRecord::Permission(record) = record else {
        return Err("stream must carry the permission record".into());
    };
    assert_eq!(
        *gate, record,
        "the snapshot and the stream record are one value"
    );
    match record {
        PermissionRecord::Requested {
            request_id: record_id,
            shows_preview,
            preview,
            ..
        } => {
            assert_eq!(record_id.as_str(), request_id.as_str());
            assert!(shows_preview);
            assert_eq!(preview.as_deref(), Some("--- a/x\n+++ b/x\n"));
        }
        other => panic!("expected a requested record, got {other:?}"),
    }
    Ok(())
}

/// A resolution rides the stream and returns the gate to idle.
#[tokio::test]
async fn settle_permission_publishes_the_record_and_closes_the_gate() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mut stream_rx = bus.stream().subscribe();
    let mut facts_rx = bus.facts().subscribe();
    let request_id = request_permission(
        bus.stream(),
        bus.facts(),
        permission_context(),
        PermissionCard::default(),
    )?;

    // -- Exec
    settle_permission(
        bus.stream(),
        bus.facts(),
        PermissionRecord::TimedOut { request_id },
    );

    // -- Check: the open card's record, then the resolution.
    let opened = timeout(Duration::from_millis(100), stream_rx.recv())
        .await
        .map_err(|_| "stream receive should not time out")?
        .map_err(|_| "stream receive should succeed")?;
    assert!(matches!(
        opened,
        SessionRecord::Permission(PermissionRecord::Requested { .. })
    ));
    let record = timeout(Duration::from_millis(100), stream_rx.recv())
        .await
        .map_err(|_| "stream receive should not time out")?
        .map_err(|_| "stream receive should succeed")?;
    assert!(matches!(
        record,
        SessionRecord::Permission(PermissionRecord::TimedOut { .. })
    ));
    // The last published snapshot is the open card; drain to the idle one.
    let mut last = None;
    while let Ok(snapshot) = facts_rx.try_recv() {
        last = Some(snapshot);
    }
    assert_eq!(
        last,
        Some(FactRecord::PermissionGate(PermissionGate::Idle)),
        "the gate returns to idle when it settles"
    );
    Ok(())
}

// -- CancelTx::request_cancel

/// `request_cancel` publishes a `Requested` cancel event; with no receiver it
/// returns an error.
#[tokio::test]
async fn request_cancel() -> Result<()> {
    // -- Case 1: a subscriber receives the cancel request
    let bus = Bus::default();
    let mut cancel_rx = bus.cancel().subscribe();
    bus.cancel().request_cancel().await?;
    let event = timeout(Duration::from_millis(100), cancel_rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(event, CancelEvent::Requested));

    // -- Case 2: no receiver returns an error
    let tx = CancelTx::new("cancel", 64);
    assert_eq!(
        tx.request_cancel().await,
        Err(ChannelError::NoReceiver),
        "send with no active receiver must return an error"
    );
    Ok(())
}
