//! The queue reaches the worker once the worker is idle.

use super::*;

use crate::catalog::PickerCatalog;
use crate::conversation::turn::TurnEnd;
use crate::orchestrator::events::UiRequest;
use crate::orchestrator::stages::UiRequestHandler;
use crate::orchestrator::stages::ui_request::UiRequestStage;

/// A switch accepted under a busy worker queues. Quit with the worker idle
/// applies it before it reads the pending set, so the effect is not dropped on
/// the way out. Quit attempts the queue once: one request reaches the worker and
/// nothing follows it.
#[tokio::test]
async fn quit_applies_the_switch_queued_under_a_stale_guard() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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
        ..
    } = h;

    // The accept runs under a busy worker, so the switch takes the queue.
    let mut stage = UiRequestStage::new(0);
    let mut queued_state = CtxState {
        worker_active: true,
        catalog: PickerCatalog::default(),
        pending: None,
    };
    {
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut queued_state,
        );
        stage
            .handle_incoming(
                UiRequest::SwitchModel {
                    spec: "queued-model".to_string(),
                },
                &mut ctx,
            )
            .await;
    }
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a switch accepted under the guard waits in the queue"
    );
    // The turn the guard named has ended: only quit follows.
    ctx_state.worker_active = false;
    event_tx.send(OrchestratorEvent::Quit).await?;

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut session = MockSession::new();

    // -- Exec
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut control_rx,
            &mut lifecycle_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut stage,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    // -- Check
    assert!(result.is_ok(), "quit exits the loop");
    match recv_command(&mut worker_rx) {
        Some(WorkerCommand::HandleUiRequest {
            request: UiRequest::SwitchModel { spec },
            ..
        }) => assert_eq!(spec, "queued-model", "the queued switch is the one applied"),
        _ => return Err("quit should dispatch the switch it found queued".into()),
    }
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "quit attempts the queue once: no second command follows the switch"
    );
    Ok(())
}

/// A wake-up that is neither a turn result nor a blocking response re-checks
/// the queue. A prompt submitted on an idle worker is such a wake-up: the
/// switch it finds queued reaches the worker instead of waiting for an event
/// that may never come.
#[tokio::test]
async fn prompt_wake_up_applies_the_switch_queued_under_a_stale_guard() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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
        ..
    } = h;

    let mut stage = UiRequestStage::new(0);
    let mut queued_state = CtxState {
        worker_active: true,
        catalog: PickerCatalog::default(),
        pending: None,
    };
    {
        let mut ctx = make_ctx(
            &worker_tx,
            &blocking_tx,
            &concurrent_tx,
            &bus,
            &mut queued_state,
        );
        stage
            .handle_incoming(
                UiRequest::SwitchAgent {
                    name: "queued-agent".to_string(),
                },
                &mut ctx,
            )
            .await;
    }
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a switch accepted under the guard waits in the queue"
    );
    // The turn the guard named has ended. A prompt wakes the loop, and quit
    // follows so the loop terminates.
    ctx_state.worker_active = false;
    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await?;
    event_tx.send(OrchestratorEvent::Quit).await?;

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut session = MockSession::new();

    // -- Exec
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut control_rx,
            &mut lifecycle_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut stage,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    // -- Check
    assert!(result.is_ok(), "quit exits the loop");
    match recv_command(&mut worker_rx) {
        Some(WorkerCommand::HandleUiRequest {
            request: UiRequest::SwitchAgent { name },
            ..
        }) => assert_eq!(name, "queued-agent", "the queued switch is the one applied"),
        _ => return Err("a prompt wake-up should dispatch the switch it found queued".into()),
    }
    Ok(())
}

/// A switch accepted while the turn's result is still queued applies when that
/// result lands: the result clears the flag and drains the queue.
#[tokio::test]
async fn switch_queued_behind_the_turn_result_applies_on_the_result() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
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
        ..
    } = h;

    let mut stage = UiRequestStage::new(0);
    ctx_state.worker_active = true;
    event_tx
        .send(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
            spec: "late-model".to_string(),
        }))
        .await?;
    worker_result_tx
        .send(TurnResult::from_end(TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }))
        .await?;
    // The closed result channel is the loop's exit once the result is read.
    drop(worker_result_tx);

    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut session = MockSession::new();

    // -- Exec
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );
    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut control_rx,
            &mut lifecycle_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut stage,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    // -- Check
    assert!(
        result.is_ok(),
        "the loop exits on the closed result channel"
    );
    match recv_command(&mut worker_rx) {
        Some(WorkerCommand::HandleUiRequest {
            request: UiRequest::SwitchModel { spec },
            ..
        }) => assert_eq!(
            spec, "late-model",
            "the queued switch applies with the result"
        ),
        _ => return Err("the turn result should drain the queued switch".into()),
    }
    Ok(())
}
