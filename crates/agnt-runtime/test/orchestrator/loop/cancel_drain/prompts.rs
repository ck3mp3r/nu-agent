//! The queued external prompts the user cancel rejects through the store.

use super::*;

use crate::conversation::turn::TurnEnd;

use std::sync::Arc;
use std::time::Duration;

use crate::bus::{BroadcastRx, Bus, CancelEvent};
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::NoExternalTaskStore;
use crate::orchestrator::handlers::CANCELLED_BY_USER_REASON;
use agnt_kernel::bus::ControlCommand;

/// The idle window the loop has to drain what the test already sent before the
/// wire frees the worker or publishes the arrival.
const INTAKE_SETTLE: Duration = Duration::from_millis(50);

/// WHEN `OrchestratorEvent::CancelRequested` lands with the worker busy, two
/// queued entries, and a recording store configured, THE `run_orchestrator_loop`
/// cancel arm SHALL call `ExternalTaskStore::reject_task` once per entry with
/// the entry id and `CANCELLED_BY_USER_REASON`.
///
/// WHEN the worker cycle is freed after such a cancel, THE loop SHALL dispatch
/// no turn for the drained entries: the prompts the queue held never run, so
/// the slash stage sees nothing. The session cancel request the arm made
/// before the drain is kept.
#[tokio::test]
async fn a_user_cancel_drains_the_queued_prompts_and_rejects_every_entry() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;
    // The worker is busy, so the two arrivals wait in the queue, and the user
    // cancel reaches the loop before the worker frees.
    ctx_state.worker_active = true;
    let mut cancel_requests = bus.cancel().subscribe();
    for (task_id, prompt) in [("task-a", "prompt a"), ("task-b", "prompt b")] {
        bus.control()
            .publish(ControlCommand::EnqueueExternalPrompt {
                task_id: task_id.to_string(),
                prompt: prompt.to_string(),
            })
            .map_err(|_| "the control channel should be open")?;
    }
    let store = Arc::new(MockExternalStore::default());

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let wire = tokio::spawn(run_cancel_drain_wire(event_tx, worker_result_tx));

    // -- Exec
    let loop_result = tokio::time::timeout(
        Duration::from_secs(5),
        run_orchestrator_loop(
            event_rx,
            SourceChannels {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: None,
                external_task_rx: None,
                external_completion_rx: None,
                external_task_store: Some(&store),
            },
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
    )
    .await
    .map_err(|_| "the orchestrator loop should exit")?;

    // -- Check
    wire.await
        .map_err(|_| "the wire task should not panic")?
        .map_err(|e| format!("the wire should complete: {e}"))?;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert_eq!(
        store.rejections(),
        vec![
            ("task-a".to_string(), CANCELLED_BY_USER_REASON.to_string()),
            ("task-b".to_string(), CANCELLED_BY_USER_REASON.to_string()),
        ],
        "the user cancel must reject every queued entry, in arrival order"
    );
    assert!(
        slash.handle_calls.is_empty(),
        "the drained entries must dispatch no turn: the slash stage saw nothing"
    );
    assert_eq!(
        cancel_request_count(&mut cancel_requests),
        1,
        "the user cancel must still request the cancel for the in-flight turn"
    );
    Ok(())
}

/// WHEN the store is absent, THE cancel arm SHALL still drain the queue without
/// erroring: the rejection is logged, and the loop keeps running.
#[tokio::test]
async fn a_user_cancel_drains_the_queue_even_without_a_store() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;
    // The worker is busy, so the arrival waits in the queue, and the user
    // cancel reaches the loop before the worker frees.
    ctx_state.worker_active = true;
    bus.control()
        .publish(ControlCommand::EnqueueExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        })
        .map_err(|_| "the control channel should be open")?;

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let wire = tokio::spawn(run_cancel_drain_wire(event_tx, worker_result_tx));

    // -- Exec
    let loop_result = tokio::time::timeout(
        Duration::from_secs(5),
        run_orchestrator_loop(
            event_rx,
            SourceChannels {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: None,
                external_task_rx: None,
                external_completion_rx: None,
                external_task_store: None::<&Arc<NoExternalTaskStore>>,
            },
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
    )
    .await
    .map_err(|_| "the orchestrator loop should exit")?;

    // -- Check
    wire.await
        .map_err(|_| "the wire task should not panic")?
        .map_err(|e| format!("the wire should complete: {e}"))?;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert!(
        slash.handle_calls.is_empty(),
        "the queued entry must be drained even with no store: it dispatched no turn"
    );
    Ok(())
}

/// WHEN the queue is empty at cancel time, THE cancel arm SHALL issue one
/// session cancel request and leave the loop able to dispatch later arrivals
/// normally.
#[tokio::test]
async fn a_user_cancel_with_an_empty_queue_keeps_the_queue_dispatchable() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx: _,
        worker_result_tx: _,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut control_rx,
        mut lifecycle_rx,
        task_cancel_rx: _,
        bus,
        mut ctx_state,
    } = h;
    let mut cancel_requests = bus.cancel().subscribe();
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let wire = tokio::spawn(run_cancel_then_arrival_wire(event_tx, bus.clone()));

    // -- Exec
    let loop_result = tokio::time::timeout(
        Duration::from_secs(5),
        run_orchestrator_loop(
            event_rx,
            SourceChannels {
                worker_result_rx: &mut worker_result_rx,
                blocking_response_rx: &mut blocking_response_rx,
                concurrent_response_rx: &mut concurrent_response_rx,
                control_rx: &mut control_rx,
                lifecycle_rx: &mut lifecycle_rx,
                task_cancel_rx: None,
                external_task_rx: None,
                external_completion_rx: None,
                external_task_store: None::<&Arc<NoExternalTaskStore>>,
            },
            Stages {
                slash: &mut slash,
                permission: &mut permission,
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        ),
    )
    .await
    .map_err(|_| "the orchestrator loop should exit")?;

    // -- Check
    wire.await
        .map_err(|_| "the wire task should not panic")?
        .map_err(|e| format!("the wire should complete: {e}"))?;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert_eq!(
        slash.handle_calls,
        vec!["prompt a".to_string()],
        "an arrival after the empty-queue cancel must dispatch normally"
    );
    assert_eq!(
        cancel_request_count(&mut cancel_requests),
        1,
        "the empty-queue cancel must reach the cancel arm: one session cancel request"
    );
    Ok(())
}

/// Drive the wire for a loop that already holds the queued arrivals: let the
/// loop process the user cancel, then free the busy worker.
async fn run_cancel_drain_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
) -> core::result::Result<(), String> {
    // The arrivals already sit in the queue; the worker must stay busy until
    // the cancel has drained them.
    tokio::time::sleep(INTAKE_SETTLE).await;
    event_tx
        .send(OrchestratorEvent::CancelRequested)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    // The cancel is on the loop's intake: the queue must be drained before the
    // worker frees, or the entries would dispatch uncancelled.
    tokio::time::sleep(INTAKE_SETTLE).await;
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;
    // The worker result is on the loop's intake: it must reach the idle state
    // before the quit, or the quit arm would request a second cancel.
    tokio::time::sleep(INTAKE_SETTLE).await;

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}

/// Drive the wire for a loop that was cancelled while its queue was empty: let
/// the loop process the cancel, then publish the arrival.
async fn run_cancel_then_arrival_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    bus: Bus,
) -> core::result::Result<(), String> {
    // The loop holds nothing, so the user cancel processes on an empty queue.
    tokio::time::sleep(INTAKE_SETTLE).await;
    event_tx
        .send(OrchestratorEvent::CancelRequested)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    // The cancel is on the loop's intake; the arrival must reach it after, so
    // the empty-queue cancel cannot suppress the next dispatch.
    tokio::time::sleep(INTAKE_SETTLE).await;
    bus.control()
        .publish(ControlCommand::EnqueueExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        })
        .map_err(|_| "the control channel should be open".to_string())?;
    tokio::time::sleep(INTAKE_SETTLE).await;

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}

/// The cancel requests the bus published. The loop requests a cancel only for
/// the turn the user cancelled, so the count is the number of requested
/// cancels.
fn cancel_request_count(cancel_requests: &mut BroadcastRx<CancelEvent>) -> usize {
    std::iter::from_fn(|| cancel_requests.try_recv().ok())
        .filter(|event| matches!(event, CancelEvent::Requested))
        .count()
}
