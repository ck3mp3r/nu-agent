use super::*;

use std::sync::Arc;
use std::time::Duration;

use crate::bus::LifecycleRx;
use crate::conversation::turn::TurnEnd;
use crate::orchestrator::SourceChannels;
use crate::orchestrator::external::NoExternalTaskStore;
use crate::orchestrator::stages::session::SessionStage;
use agnt_kernel::bus::{ControlCommand, LifecycleEvent};

/// One arrival: the task, the prompt its caller sent, and the answer its turn
/// produces.
const TASKS: [(&str, &str, &str); 3] = [
    ("task-a", "prompt a", "answer a"),
    ("task-b", "prompt b", "answer b"),
    ("task-c", "prompt c", "answer c"),
];

/// WHEN three external prompts arrive back to back, THE loop SHALL run one turn
/// per task in arrival order and SHALL publish one `TaskCompleted` per task,
/// each carrying the answer of the prompt it dispatched.
///
/// The surface submits nothing for an external arrival: the loop's own queue is
/// the single intake, so no unattributed echo runs behind the attributed turns.
#[tokio::test]
async fn three_stacked_external_turns_keep_their_own_answers() -> Result<()> {
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
    // Three arrivals, back to back, before any turn runs.
    for (task_id, prompt, _) in TASKS {
        bus.control()
            .publish(ControlCommand::EnqueueExternalPrompt {
                task_id: task_id.to_string(),
                prompt: prompt.to_string(),
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

    // The worker side of the wire: the dispatches the loop makes and the
    // results the worker sends back, in the order the worker runs them. It reads
    // the spine on its own subscription, so the loop keeps the one it folds.
    let wire = tokio::spawn(run_wire(
        event_tx,
        worker_result_tx,
        worker_rx,
        bus.lifecycle().subscribe(),
    ));

    // -- Exec: run the loop while the wire drives the worker side.
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
    let completions = wire
        .await
        .map_err(|_| "the wire task should not panic")?
        .map_err(|e| format!("the wire should complete: {e}"))?;

    // -- Check: one completion per task, in arrival order, each its own answer.
    assert_eq!(
        completions,
        TASKS
            .iter()
            .map(|(task_id, _, answer)| (task_id.to_string(), answer.to_string()))
            .collect::<Vec<_>>(),
        "each task must complete with the answer of the turn it dispatched"
    );
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    Ok(())
}

/// Drive the worker side of the wire: read each dispatch the loop makes, check
/// the attribution it carries, answer it, and collect the completions.
async fn run_wire(
    event_tx: mpsc::Sender<OrchestratorEvent>,
    worker_result_tx: mpsc::Sender<TurnResult>,
    mut worker_rx: Option<mpsc::Receiver<WorkerCommand>>,
    mut lifecycle_rx: LifecycleRx,
) -> core::result::Result<Vec<(String, String)>, String> {
    for (task_id, prompt, answer) in TASKS {
        match recv_worker_command(&mut worker_rx, &format!("ExecuteTurn for {task_id}"))
            .await
            .map_err(|e| e.to_string())?
        {
            WorkerCommand::ExecuteTurn {
                prompt: dispatched,
                task_id: dispatched_task_id,
                ..
            } => {
                if dispatched != prompt {
                    return Err(format!("{task_id} must dispatch its own prompt"));
                }
                if dispatched_task_id.as_deref() != Some(task_id) {
                    return Err(format!("{task_id} must dispatch its own attribution"));
                }
            }
            _ => return Err(format!("expected an ExecuteTurn for {task_id}")),
        }
        // This turn finishes before the loop dispatches the next.
        worker_result_tx
            .send(
                TurnResult::from_end(TurnEnd::Completed {
                    value: Value::string(answer, Span::test_data()),
                })
                .with_task_id(Some(task_id.to_string())),
            )
            .await
            .map_err(|_| "the worker result channel should be open".to_string())?;
    }

    let mut completions: Vec<(String, String)> = Vec::new();
    while completions.len() < TASKS.len() {
        let event = tokio::time::timeout(Duration::from_secs(5), lifecycle_rx.recv())
            .await
            .map_err(|_| format!("expected three task completions, got {completions:?}"))?
            .map_err(|_| "the lifecycle channel should stay open".to_string())?;
        if let LifecycleEvent::TaskCompleted { output, task_id } = event {
            completions.push((task_id, output));
        }
    }

    // The loop is idle, so a quit ends it.
    event_tx
        .send(OrchestratorEvent::Quit)
        .await
        .map_err(|_| "the event channel should be open".to_string())?;
    Ok(completions)
}
