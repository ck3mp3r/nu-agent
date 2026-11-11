use super::*;

use crate::conversation::turn::TurnEnd;

use std::sync::Arc;
use std::time::Duration;

use crate::bus::LifecycleRx;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::NoExternalTaskStore;
use crate::orchestrator::stages::session::SessionStage;
use agnt_kernel::bus::{ControlCommand, LifecycleEvent};

/// The quiet window in which no further turn may be dispatched.
const NO_COMMAND_BUDGET: Duration = Duration::from_millis(50);

/// WHEN a repeated arrival names a queued or in-flight task, THE loop SHALL
/// admit nothing for it and SHALL run no second turn for that id.
///
/// The repetition is a notification, not a new task: the queue already holds
/// the id, so the repeat is logged and dropped instead of handing one task a
/// second turn whose answer would overwrite the first.
#[tokio::test]
async fn a_repeated_arrival_runs_no_second_turn() -> Result<()> {
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
        mut task_cancel_rx,
        bus,
        mut ctx_state,
    } = h;
    // Two intakes of the same task, back to back on the control bus.
    for _ in 0..2 {
        bus.control()
            .publish(ControlCommand::EnqueueExternalPrompt {
                task_id: "task-a".to_string(),
                prompt: "prompt a".to_string(),
            })
            .map_err(|_| "the control channel should be open")?;
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

    // -- Exec
    let wire = tokio::spawn(run_wire(
        event_tx,
        worker_result_tx,
        worker_rx,
        bus.lifecycle().subscribe(),
    ));
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
                task_cancel_rx: task_cancel_rx.take(),
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
    Ok(())
}

/// Drive the worker side of the wire: one turn for the task, then a quiet window.
async fn run_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    mut worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    mut lifecycle_rx: LifecycleRx,
) -> core::result::Result<(), String> {
    match recv_worker_command(&mut worker_rx, "ExecuteTurn for task-a")
        .await
        .map_err(|e| e.to_string())?
    {
        WorkerCommand::ExecuteTurn { task_id, .. } => {
            if task_id.as_deref() != Some("task-a") {
                return Err("the one turn must carry the task's own attribution".to_string());
            }
        }
        _ => return Err("expected an ExecuteTurn for task-a".to_string()),
    }

    // The turn finishes: the queue is free and holds nothing else for task-a.
    worker_result_tx
        .send(
            TurnResult::from_end(TurnEnd::Completed {
                value: Value::string("answer a", Span::test_data()),
            })
            .with_task_id(Some("task-a".to_string())),
        )
        .await
        .map_err(|_| "the worker result channel should be open".to_string())?;

    // Exactly one completion, carrying the one turn's answer.
    let mut completions: Vec<(String, String)> = Vec::new();
    while completions.is_empty() {
        let event = tokio::time::timeout(Duration::from_secs(5), lifecycle_rx.recv())
            .await
            .map_err(|_| "task-a should complete".to_string())?
            .map_err(|_| "the lifecycle channel should stay open".to_string())?;
        if let LifecycleEvent::TaskCompleted { output, task_id } = event {
            completions.push((task_id, output));
        }
    }
    if completions.len() != 1 {
        return Err(format!("expected one completion, got {completions:?}"));
    }

    // The repeated arrival must not dispatch a turn of its own.
    let rx = worker_rx
        .as_mut()
        .ok_or("the worker channel should be present")?;
    if tokio::time::timeout(NO_COMMAND_BUDGET, rx.recv())
        .await
        .is_ok()
    {
        return Err("the repeated arrival must not dispatch a second turn".to_string());
    }

    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(())
}
