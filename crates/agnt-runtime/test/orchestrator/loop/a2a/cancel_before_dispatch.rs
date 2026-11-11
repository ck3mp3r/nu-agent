use super::*;

use crate::conversation::turn::TurnEnd;

use std::sync::Arc;
use std::time::Duration;

use crate::bus::{BroadcastRx, CancelEvent, LifecycleRx};
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::NoExternalTaskStore;
use crate::orchestrator::stages::session::{SessionStage, TASK_CANCELLED_NOTICE};
use agnt_kernel::bus::{ControlCommand, LifecycleEvent};

/// The idle window the loop has to drain what the test already sent before the
/// wire frees the worker or publishes the arrival.
const INTAKE_SETTLE: Duration = Duration::from_millis(50);

/// WHEN a cancel names a task before that task's prompt is dispatched, THE loop
/// SHALL publish `TaskFailed` for that task id once its turn begins.
///
/// The task's prompt waits in the loop's queue while another turn runs. The
/// turn the prompt starts is the one the cancel stops, and the failure it
/// reports carries the id of the task the cancel named.
#[tokio::test]
async fn a_cancel_before_dispatch_fails_the_task_it_names() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
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
    // The worker is busy, so the arrival waits in the queue instead of being
    // dispatched, and the cancel reaches the loop before that dispatch.
    ctx_state.worker_active = true;
    let mut cancel_requests = bus.cancel().subscribe();
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel::<String>();
    bus.control()
        .publish(ControlCommand::EnqueueExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        })
        .map_err(|_| "the control channel should be open")?;
    cancel_tx
        .send("task-a".to_string())
        .map_err(|_| "the cancel channel should be open")?;

    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = SessionStage;
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let wire = tokio::spawn(run_wire(
        event_tx,
        worker_result_tx,
        worker_rx,
        bus.lifecycle().subscribe(),
    ));

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
                task_cancel_rx: Some(cancel_rx),
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
        cancel_request_count(&mut cancel_requests),
        1,
        "the waiting cancel must be requested exactly once, at the dispatch it named"
    );
    Ok(())
}

/// WHEN a before-dispatch cancel arrives more than once for the same queued
/// task, THE queue SHALL honour it once.
///
/// Both cancels name a task whose turn has not begun. The queue holds one
/// waiting cancel, so the dispatch it names requests one cancel — not one per
/// notification.
#[tokio::test]
async fn a_repeated_before_dispatch_cancel_is_honoured_once() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
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
    // The worker is busy, so the arrival waits in the queue while both cancels
    // reach the loop.
    ctx_state.worker_active = true;
    let mut cancel_requests = bus.cancel().subscribe();
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel::<String>();
    bus.control()
        .publish(ControlCommand::EnqueueExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        })
        .map_err(|_| "the control channel should be open")?;
    for _ in 0..2 {
        cancel_tx
            .send("task-a".to_string())
            .map_err(|_| "the cancel channel should be open")?;
    }

    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = SessionStage;
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let wire = tokio::spawn(run_wire_releasing_the_worker(
        event_tx,
        worker_result_tx,
        worker_rx,
        bus.lifecycle().subscribe(),
    ));

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
                task_cancel_rx: Some(cancel_rx),
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
        cancel_request_count(&mut cancel_requests),
        1,
        "a repeated cancel must be honoured once, not once per notification"
    );
    Ok(())
}

/// WHEN a cancel names a task that has not arrived, THE queue SHALL hold the
/// cancel and stop that task's turn at its arrival.
///
/// The cancel reaches the loop first. The queue does not know the task, so the
/// cancel waits; the arrival that follows is dispatched into a turn cancelled at
/// entry, and the task reaches its terminal row.
#[tokio::test]
async fn a_cancel_before_the_task_arrives_is_honoured_at_its_dispatch() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_rx,
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
    let mut cancel_requests = bus.cancel().subscribe();
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel::<String>();
    cancel_tx
        .send("task-a".to_string())
        .map_err(|_| "the cancel channel should be open")?;

    let mut slash = SlashStage;
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = SessionStage;
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let wire = tokio::spawn(run_wire_arriving_late(
        event_tx,
        worker_result_tx,
        worker_rx,
        bus.lifecycle().subscribe(),
        bus.clone(),
    ));

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
                task_cancel_rx: Some(cancel_rx),
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
        cancel_request_count(&mut cancel_requests),
        1,
        "the cancel that waited for an unarrived task must be requested at its dispatch"
    );
    Ok(())
}

/// Drive the worker side of the wire: free the busy worker, run the one turn the
/// queue then dispatches, and report it as cancelled.
async fn run_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    mut lifecycle_rx: LifecycleRx,
) -> core::result::Result<(), String> {
    // The turn already in flight finishes, freeing the queue.
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;

    run_cancelled_turn(&worker_result_tx, worker_rx, &mut lifecycle_rx).await?;

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}

/// Drive the wire for a loop that already holds the arrival and its cancels:
/// wait for the loop to drain them, then free the busy worker.
async fn run_wire_releasing_the_worker(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    mut lifecycle_rx: LifecycleRx,
) -> core::result::Result<(), String> {
    // The cancels are already on the loop's intake; freeing the worker before it
    // drains them would dispatch the arrival uncancelled.
    tokio::time::sleep(INTAKE_SETTLE).await;
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;

    run_cancelled_turn(&worker_result_tx, worker_rx, &mut lifecycle_rx).await?;

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}

/// Drive the wire for a loop that was cancelled before the task arrived: let the
/// loop hold the cancel, then publish the arrival.
async fn run_wire_arriving_late(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    mut lifecycle_rx: LifecycleRx,
    bus: crate::bus::Bus,
) -> core::result::Result<(), String> {
    // The cancel is already on the loop's intake; the arrival must reach it
    // after, so the cancel waits for a task the queue does not know yet.
    tokio::time::sleep(INTAKE_SETTLE).await;
    bus.control()
        .publish(ControlCommand::EnqueueExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        })
        .map_err(|_| "the control channel should be open".to_string())?;

    run_cancelled_turn(&worker_result_tx, worker_rx, &mut lifecycle_rx).await?;

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}

/// Expect the one attributed turn the queue dispatches, report it cancelled, and
/// check that its task reached its terminal row.
async fn run_cancelled_turn(
    worker_result_tx: &mpsc::Sender<TurnResult>,
    mut worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    lifecycle_rx: &mut LifecycleRx,
) -> core::result::Result<(), String> {
    match recv_worker_command(&mut worker_rx, "ExecuteTurn for task-a")
        .await
        .map_err(|e| e.to_string())?
    {
        WorkerCommand::ExecuteTurn { task_id, .. } => {
            if task_id.as_deref() != Some("task-a") {
                return Err("the cancelled turn must carry the task's own attribution".to_string());
            }
        }
        _ => return Err("expected an ExecuteTurn for task-a".to_string()),
    }

    // The turn the cancel stops reports itself cancelled, as the executor's
    // typed end — no message is parsed to say so.
    worker_result_tx
        .send(
            TurnResult::from_end(TurnEnd::Cancelled {
                value: Value::nothing(Span::test_data()),
            })
            .with_task_id(Some("task-a".to_string())),
        )
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;

    let mut failures: Vec<(String, String)> = Vec::new();
    while failures.is_empty() {
        let event = tokio::time::timeout(Duration::from_secs(5), lifecycle_rx.recv())
            .await
            .map_err(|_| "the cancelled task should fail".to_string())?
            .map_err(|_| "the lifecycle channel should stay open".to_string())?;
        if let LifecycleEvent::TaskFailed { task_id, error } = event {
            failures.push((task_id, error));
        }
    }
    if failures.len() != 1 {
        return Err(format!("expected one task failure, got {failures:?}"));
    }
    let (task_id, error) = &failures[0];
    if task_id != "task-a" || error != TASK_CANCELLED_NOTICE {
        return Err(format!("unexpected failure row {failures:?}"));
    }
    Ok(())
}

/// The cancel requests the bus published. The loop requests a cancel only for a
/// turn it cancelled, so the count is the number of honoured cancels.
fn cancel_request_count(cancel_requests: &mut BroadcastRx<CancelEvent>) -> usize {
    std::iter::from_fn(|| cancel_requests.try_recv().ok())
        .filter(|event| matches!(event, CancelEvent::Requested))
        .count()
}
