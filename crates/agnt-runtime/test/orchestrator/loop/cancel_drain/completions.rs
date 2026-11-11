//! The buffered completions the user cancel abandons.

use super::*;

use crate::conversation::turn::TurnEnd;

use std::sync::Arc;
use std::time::Duration;

use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::{ExternalCompletion, NoExternalTaskStore};

/// The idle window the loop has to drain what the test already sent before the
/// wire frees the worker or publishes the arrival.
const INTAKE_SETTLE: Duration = Duration::from_millis(50);

/// WHEN `OrchestratorEvent::CancelRequested` lands with one buffered completion
/// and the worker busy, THE `run_orchestrator_loop` cancel arm SHALL empty the
/// completion buffer, so no completion turn dispatches after the worker frees.
///
/// WHEN a fresh completion arrives after such a cancel with the worker idle,
/// THE `external_completion_rx` arm SHALL continue its normal dispatch.
#[tokio::test]
async fn a_user_cancel_drains_the_buffered_completions_and_the_next_arrival_dispatches()
-> Result<()> {
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
    // The worker is busy, so the completion waits in the buffer, and the user
    // cancel reaches the loop before the worker frees.
    ctx_state.worker_active = true;
    let (completion_tx, completion_rx) = mpsc::channel::<ExternalCompletion>(16);
    completion_tx
        .send(ExternalCompletion {
            task_id: "task-a".to_string(),
            prompt: "remote task-a completed".to_string(),
        })
        .await
        .map_err(|_| "the completion channel should be open")?;

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
    let wire = tokio::spawn(run_completions_drain_wire(
        event_tx,
        worker_result_tx,
        completion_tx,
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
                task_cancel_rx: None,
                external_task_rx: None,
                external_completion_rx: Some(completion_rx),
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
        vec!["remote task-b completed".to_string()],
        "the cancelled completion must dispatch no turn, and a fresh idle arrival must dispatch normally"
    );
    Ok(())
}

/// Drive the wire for a loop that already holds the buffered completion: let
/// the loop process the user cancel, free the busy worker, then send the fresh
/// completion the cancel leaves in normal flow.
async fn run_completions_drain_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    completion_tx: mpsc::Sender<ExternalCompletion>,
) -> core::result::Result<(), String> {
    // The completion already sits in the buffer; the worker must stay busy
    // until the cancel has drained it.
    tokio::time::sleep(INTAKE_SETTLE).await;
    event_tx
        .send(OrchestratorEvent::CancelRequested)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    // The cancel is on the loop's intake: the buffer must be drained before
    // the worker frees, or the buffered completion would dispatch uncancelled.
    tokio::time::sleep(INTAKE_SETTLE).await;
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;
    // The worker result is on the loop's intake: the drained buffer must be
    // through the idle cycle before the fresh arrival, so the arrival meets
    // the normal dispatch path the user cancel must keep.
    tokio::time::sleep(INTAKE_SETTLE).await;
    completion_tx
        .send(ExternalCompletion {
            task_id: "task-b".to_string(),
            prompt: "remote task-b completed".to_string(),
        })
        .await
        .map_err(|_| "the completion channel should be open".to_string())?;
    tokio::time::sleep(INTAKE_SETTLE).await;

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}
