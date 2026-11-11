use super::*;

use std::sync::Arc;

use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::ExternalTask;
use crate::orchestrator::handlers::WORKER_BUSY_REASON;
use crate::protocol::event::{PermissionDecision, PermissionDecisionSubmission};

// ---------------------------------------------------------------------------
// Saturated worker command channel
// ---------------------------------------------------------------------------

/// An external task that arrives while the worker command channel is at
/// capacity must be rejected instead of blocking the loop on `send`, and the
/// loop must keep draining the TUI event channel.
///
/// This is the compaction stall: the worker is busy but `worker_active` is
/// false, so the intake arm is reached, and an awaited `send` on a full channel
/// parks the loop — which stops draining `event_rx` and freezes TUI input until
/// compaction ends.
#[tokio::test]
async fn external_task_rx_rejects_when_worker_channel_full_and_keeps_draining_events() -> Result<()>
{
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
    } = h;

    let (external_task_tx, external_task_rx) = mpsc::channel::<ExternalTask>(16);
    let store = Arc::new(MockExternalStore::default());
    let loop_store = Arc::clone(&store);

    // Saturate the worker command channel. Nothing drains it until the scenario
    // ends, so it stays full throughout.
    let filler_tx = worker_tx.clone();
    for _ in 0..WORKER_CMD_CAPACITY {
        filler_tx
            .send(WorkerCommand::NewSession)
            .await
            .map_err(|_| "the worker channel should accept a filler command")?;
    }
    assert!(
        matches!(
            filler_tx.try_send(WorkerCommand::NewSession),
            Err(mpsc::error::TrySendError::Full(_))
        ),
        "the worker channel must be at capacity before the task arrives"
    );

    let trace = Trace::default();
    let loop_trace = Arc::clone(&trace);
    let loop_task = tokio::spawn(async move {
        let mut slash = SlashStage;
        let mut permission = MockPermission::with_trace(loop_trace);
        let mut ui_request = MockUiRequest::new();
        let mut session = MockSession::new();
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut ctx_state,
        );
        let sources = SourceChannels {
            worker_result_rx: &mut worker_result_rx,
            blocking_response_rx: &mut blocking_response_rx,
            concurrent_response_rx: &mut concurrent_response_rx,
            control_rx: &mut control_rx,
            lifecycle_rx: &mut lifecycle_rx,
            task_cancel_rx: task_cancel_rx.take(),
            external_task_rx: Some(external_task_rx),
            external_completion_rx: None,
            external_task_store: Some(&loop_store),
        };
        run_orchestrator_loop(
            event_rx,
            sources,
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    // -- Exec
    // The task arrives while the worker command channel is full. The intake arm
    // must reject it rather than await capacity; an awaited send parks the loop
    // and this wait expires.
    external_task_tx
        .send(external_task("task-1", "ctx-1", "[A2A] storm"))
        .await
        .map_err(|_| "the external task send should succeed")?;
    let (_, reason) = wait_for_rejection(&store, "task-1").await?;

    // The loop is still live: a permission decision submitted while the worker
    // channel is full must reach the permission resolver.
    event_tx
        .send(OrchestratorEvent::PermissionDecision {
            decision: PermissionDecisionSubmission {
                request_id: "req-1".to_string(),
                decision: PermissionDecision::AllowOnce,
            },
        })
        .await
        .map_err(|_| "the event channel should be open")?;
    let permission_handled = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let seen = trace
                .lock()
                .map(|entries| entries.iter().any(|entry| entry == "permission"))
                .unwrap_or(false);
            if seen {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;

    // -- Check
    assert_eq!(
        reason, WORKER_BUSY_REASON,
        "the rejected task must carry the busy reason"
    );
    assert!(
        permission_handled.is_ok(),
        "the loop must keep draining the TUI event channel while the worker channel is full"
    );

    // Quit so the loop exits cleanly.
    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the quit send should succeed")?;
    let result = loop_task
        .await
        .map_err(|e| format!("the loop task should not panic: {e}"))?;
    assert!(result.is_ok());

    // No attach command was queued: the channel still holds only fillers.
    let rx = worker_rx.as_mut().ok_or("worker_rx present")?;
    let mut dispatched_attach = false;
    while let Ok(cmd) = rx.try_recv() {
        if matches!(cmd, WorkerCommand::AttachA2aContext { .. }) {
            dispatched_attach = true;
        }
    }
    assert!(
        !dispatched_attach,
        "the loop must not queue an attach command on a full channel"
    );
    Ok(())
}
