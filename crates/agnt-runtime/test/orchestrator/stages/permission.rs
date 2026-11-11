//! Unit tests for the permission stage.
//!
//! The stage forwards a submitted decision to the pending map. A submission
//! for a request that is no longer pending is dropped, so the stage publishes
//! `PermissionRecord::Ignored` to make the drop observable.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nu_protocol::Span;
use tokio::sync::mpsc;

use crate::bus::{Bus, OneshotRx, OneshotTx};
use crate::conversation::runtime::PendingPermissions;
use crate::orchestrator::events::{UiRequestResponse, WorkerCommand};
use crate::orchestrator::stages::permission::PermissionStage;
use crate::orchestrator::stages::{OrchestrationContext, PermissionHandler};
use crate::protocol::event::{PermissionDecision, PermissionDecisionSubmission};
use agnt_kernel::{PermissionRecord, SessionRecord};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixture

/// Everything `OrchestrationContext` borrows, owned by the test so `submit`
/// can build a fresh context per call.
struct Fixture {
    stage: PermissionStage,
    worker_tx: mpsc::Sender<WorkerCommand>,
    blocking_tx: mpsc::Sender<UiRequestResponse>,
    concurrent_tx: mpsc::Sender<UiRequestResponse>,
    bus: Bus,
    stream_rx: agnt_kernel::bus::Subscription<agnt_kernel::SessionRecord>,
    catalog: crate::catalog::PickerCatalog,
    worker_active: bool,
    pending: Option<PendingPermissions>,
}

impl Fixture {
    fn new(pending: Option<PendingPermissions>) -> Self {
        let (worker_tx, _worker_rx) = mpsc::channel::<WorkerCommand>(8);
        let (blocking_tx, _blocking_rx) = mpsc::channel::<UiRequestResponse>(8);
        let (concurrent_tx, _concurrent_rx) = mpsc::channel::<UiRequestResponse>(8);
        let bus = Bus::default();
        let stream_rx = bus.stream().subscribe();
        Self {
            stage: PermissionStage,
            worker_tx,
            blocking_tx,
            concurrent_tx,
            bus,
            stream_rx,
            catalog: crate::catalog::PickerCatalog::default(),
            worker_active: false,
            pending,
        }
    }

    async fn submit(&mut self, submission: PermissionDecisionSubmission) {
        let mut ctx = OrchestrationContext {
            worker_tx: &self.worker_tx,
            blocking_response_tx: &self.blocking_tx,
            concurrent_response_tx: &self.concurrent_tx,
            pending: &self.pending,
            worker_active: &mut self.worker_active,
            catalog: &mut self.catalog,
            span: Span::test_data(),
            stream: self.bus.stream(),
            control: self.bus.control(),
            cancel: self.bus.cancel(),
            lifecycle: self.bus.lifecycle(),
            narration: self.bus.narration(),
            facts: self.bus.facts(),
        };
        self.stage.handle(submission, &mut ctx).await;
    }

    /// Every `PermissionDecisionIgnored` event published so far, as
    /// `(request_id, reason)`.
    fn take_ignored(&mut self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        while let Ok(record) = self.stream_rx.try_recv() {
            if let SessionRecord::Permission(PermissionRecord::Ignored { request_id, reason }) =
                record
            {
                out.push((request_id.as_str().to_string(), reason));
            }
        }
        out
    }
}

fn submission(request_id: &str, decision: PermissionDecision) -> PermissionDecisionSubmission {
    PermissionDecisionSubmission {
        request_id: request_id.to_string(),
        decision,
    }
}

fn empty_pending() -> PendingPermissions {
    Arc::new(Mutex::new(HashMap::new()))
}

fn register_pending(
    pending: &PendingPermissions,
    request_id: &str,
) -> OneshotRx<PermissionDecision> {
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("permission");
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(request_id.to_string(), tx);
    rx
}

// endregion: --- Fixture

// region:    --- Tests

#[tokio::test]
async fn test_orchestrator_permission_stage_unknown_request_publishes_ignored() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = Fixture::new(Some(empty_pending()));

    // -- Exec
    fixture
        .submit(submission("req-1", PermissionDecision::AllowOnce))
        .await;

    // -- Check
    assert_eq!(
        fixture.take_ignored(),
        vec![("req-1".to_string(), "request_not_pending".to_string())],
        "a submission for an unknown request must publish one ignored event"
    );
    Ok(())
}

#[tokio::test]
async fn test_orchestrator_permission_stage_pending_request_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let pending = empty_pending();
    let rx = register_pending(&pending, "req-1");
    let mut fixture = Fixture::new(Some(pending));

    // -- Exec
    fixture
        .submit(submission("req-1", PermissionDecision::Deny))
        .await;
    let resolved = rx.await?;

    // -- Check
    assert_eq!(resolved, PermissionDecision::Deny);
    assert!(
        fixture.take_ignored().is_empty(),
        "a submission for a pending request must not publish an ignored event"
    );
    Ok(())
}

#[tokio::test]
async fn test_orchestrator_permission_stage_without_pending_map_publishes_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = Fixture::new(None);

    // -- Exec
    fixture
        .submit(submission("req-1", PermissionDecision::AllowOnce))
        .await;

    // -- Check
    assert!(
        fixture.take_ignored().is_empty(),
        "no permission resolver means no pending map, so no ignored event"
    );
    Ok(())
}

// endregion: --- Tests
