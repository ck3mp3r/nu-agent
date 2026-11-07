use super::*;

use crate::orchestrator::handlers::WORKER_BUSY_REASON;
use crate::protocol::event::{PermissionDecision, PermissionDecisionSubmission};

// ---------------------------------------------------------------------------
// Saturated worker command channel
// ---------------------------------------------------------------------------

/// An incoming A2A task that arrives while the worker command channel is at
/// capacity must be rejected instead of blocking the loop on `send`, and the
/// loop must keep draining the TUI event channel.
///
/// This is the compaction stall: the worker is busy but `worker_active` is
/// false, so the intake arm is reached, and an awaited `send` on a full channel
/// parks the loop — which stops draining `event_rx` and freezes TUI input until
/// compaction ends.
#[tokio::test]
async fn a2a_task_rx_rejects_when_worker_channel_full_and_keeps_draining_events() -> Result<()> {
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
        mut external_rx,
        mut compaction_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;

    let (a2a_task_tx, a2a_task_rx) = mpsc::channel::<IncomingTask>(16);

    // The server handler transitions an incoming task Submitted→Working before
    // the orchestrator sees it. Model that here so the rejection has a Working
    // task to transition.
    let store = std::sync::Arc::new(InMemoryTaskStore::default());
    let task = store.create_task(None, None, None);
    store
        .update_status(&task.id, TaskState::Working, None)
        .map_err(|e| format!("the incoming task should reach Working: {e}"))?;
    let task_id = task.id.clone();
    ctx_state.task_store = Some(std::sync::Arc::clone(&store));

    // Saturate the worker command channel. Nothing drains it until the scenario
    // ends, so it stays full throughout.
    let filler_tx = worker_tx.clone();
    for _ in 0..WORKER_CMD_CAPACITY {
        filler_tx
            .send(WorkerCommand::ClearSession)
            .await
            .map_err(|_| "the worker channel should accept a filler command")?;
    }
    assert!(
        matches!(
            filler_tx.try_send(WorkerCommand::ClearSession),
            Err(mpsc::error::TrySendError::Full(_))
        ),
        "the worker channel must be at capacity before the task arrives"
    );

    let trace = Trace::default();
    let loop_trace = std::sync::Arc::clone(&trace);
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
        let mut task_cancel_rx: Option<mpsc::UnboundedReceiver<String>> = None;
        let mut sources = make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        );
        sources.a2a_task_rx = Some(a2a_task_rx);
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
    a2a_task_tx
        .send(incoming_task(&task_id, "storm"))
        .await
        .map_err(|_| "the incoming task send should succeed")?;
    wait_for_task_state(&store, &task_id, TaskState::Rejected).await?;

    // The loop is still live: a permission decision submitted while the worker
    // channel is full must reach the permission resolver.
    event_tx
        .send(OrchestratorEvent::PermissionDecision {
            decision: PermissionDecisionSubmission {
                request_id: "req-1".to_string(),
                decision: PermissionDecision::AllowOnce,
                matched_rule_identity: "rule-1".to_string(),
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
    let rejected = store
        .get_task(&task_id)
        .map_err(|e| format!("the rejected task should exist: {e}"))?;
    assert_eq!(rejected.status.state, TaskState::Rejected);
    let message = rejected
        .status
        .message
        .ok_or("the rejected task should carry a status message")?;
    let text = match message.parts.first() {
        Some(Part::Text { text }) => text.clone(),
        _ => return Err("the status message should carry a text part".into()),
    };
    assert_eq!(
        text, WORKER_BUSY_REASON,
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
