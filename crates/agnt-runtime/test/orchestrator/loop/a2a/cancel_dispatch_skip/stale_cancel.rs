//! The stale cancel a skipped dispatch leaves behind.

use super::*;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::bus::{BroadcastRx, Bus, CancelEvent};
use crate::conversation::turn::TurnEnd;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::ExternalTaskStore;
use crate::orchestrator::stages::session::SessionStage;
use agnt_kernel::bus::ControlCommand;

/// A store whose named task answers terminal exactly once, then answers live.
///
/// The first dispatch of the task is skipped; the re-enqueued task is a live
/// store row again, so its second dispatch runs a turn.
#[derive(Default)]
struct TerminalOnce {
    pending: Mutex<Option<String>>,
}

impl TerminalOnce {
    /// A store that reports `task_id` as terminal on its first state read only.
    fn holding_terminal_once(task_id: &str) -> Self {
        Self {
            pending: Mutex::new(Some(task_id.to_string())),
        }
    }
}

impl ExternalTaskStore for TerminalOnce {
    fn reject_task(&self, _task_id: &str, _reason: &str) -> core::result::Result<(), String> {
        Ok(())
    }

    fn is_task_terminal(&self, task_id: &str) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.as_deref() == Some(task_id) {
            *pending = None;
            return true;
        }
        false
    }
}

/// WHEN a cancel waits for a task whose dispatch is skipped as terminal, THE
/// loop SHALL NOT publish a cancel request when that task's turn later runs.
///
/// The caller was already told the task ended, so a cancel published at the
/// later dispatch names a turn nobody is waiting on. The skip is the moment the
/// waiting cancel can no longer stop anything, so the queue drops it there.
#[tokio::test]
async fn a_cancel_that_waited_for_a_skipped_task_does_not_fire_at_the_later_dispatch() -> Result<()>
{
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
    // The worker is busy, so the arrival queues, and the cancel reaches the loop
    // while the prompt waits for its dispatch.
    ctx_state.worker_active = true;
    let store = Arc::new(TerminalOnce::holding_terminal_once("task-a"));
    let mut cancel_requests = bus.cancel().subscribe();
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel::<String>();
    enqueue(&bus, &[("task-a", "prompt a")])?;
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
    let wire = tokio::spawn(run_wire_requeueing(
        event_tx,
        worker_result_tx,
        worker_rx,
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
    let dispatched = wire.await.map_err(|_| "the wire task should not panic")??;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert_eq!(
        dispatched,
        Some("task-a".to_string()),
        "the re-enqueued task must run its turn once the store reports it live"
    );
    assert_eq!(
        cancel_request_count(&mut cancel_requests),
        0,
        "a cancel that waited for a skipped task must not fire at the later dispatch"
    );
    Ok(())
}

/// Drive the wire: free the busy worker so the queued task is reached and
/// skipped, re-enqueue it as a live task, run that turn, and quit the loop.
async fn run_wire_requeueing(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    mut worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    bus: Bus,
) -> core::result::Result<Option<String>, String> {
    // The arrival and its cancel are already on the loop's intake.
    tokio::time::sleep(INTAKE_SETTLE).await;
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;

    // The skipped dispatch ran no turn. Once the loop is idle again, the same
    // task arrives against a live store row.
    tokio::time::sleep(INTAKE_SETTLE).await;
    bus.control()
        .publish(ControlCommand::EnqueueExternalPrompt {
            task_id: "task-a".to_string(),
            prompt: "prompt a".to_string(),
        })
        .map_err(|_| "the control channel should be open".to_string())?;

    let next = tokio::time::timeout(DISPATCH_SETTLE, async {
        match worker_rx.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    })
    .await
    .ok()
    .flatten();
    let dispatched = match next {
        Some(WorkerCommand::ExecuteTurn { task_id, .. }) => task_id,
        _ => None,
    };

    if let Some(task_id) = &dispatched {
        let ended = TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        })
        .with_task_id(Some(task_id.clone()));
        worker_result_tx
            .send(ended)
            .await
            .map_err(|_| "the worker result channel should be open".to_string())?;
        // Let the loop fold that end before the quit arrives: a quit that lands
        // on a running turn cancels it, and that request is not the stale one
        // this test counts.
        tokio::time::sleep(INTAKE_SETTLE).await;
    }

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(dispatched)
}

/// The cancel requests the bus published. The loop requests a cancel only for a
/// turn it cancelled, so the count is the number of honoured cancels.
fn cancel_request_count(cancel_requests: &mut BroadcastRx<CancelEvent>) -> usize {
    std::iter::from_fn(|| cancel_requests.try_recv().ok())
        .filter(|event| matches!(event, CancelEvent::Requested))
        .count()
}
