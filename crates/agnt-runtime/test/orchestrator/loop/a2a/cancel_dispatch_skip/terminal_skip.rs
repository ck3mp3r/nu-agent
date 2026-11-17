//! The dispatch-time skip itself: what the loop does with a terminal store row.

use super::*;

use std::sync::Arc;
use std::time::Duration;

use crate::orchestrator::SourceChannels;
use crate::orchestrator::stages::session::SessionStage;

/// WHEN the loop reaches a queued external prompt whose store state is
/// terminal, THE loop SHALL NOT submit a turn for that task.
///
/// A cancel reaches the store before its signal reaches the loop, so the task
/// is terminal before its turn begins and no executor of that turn could
/// observe the cancel. The dispatch reads the store row and skips the entry.
#[tokio::test]
async fn a_terminal_task_is_not_dispatched() -> Result<()> {
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
    // The worker is busy, so the arrival waits in the queue while its store row
    // is already terminal.
    ctx_state.worker_active = true;
    let store = Arc::new(StagedStore::holding_terminal(&["task-a"]));
    enqueue(&bus, &[("task-a", "prompt a")])?;

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
    let wire = tokio::spawn(run_wire(event_tx, worker_result_tx, worker_rx));

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
    let dispatched = wire.await.map_err(|_| "the wire task should not panic")??;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert_eq!(
        dispatched, None,
        "a task the store already holds as terminal must run no turn"
    );
    assert!(
        store.rejections().is_empty(),
        "a skipped terminal task must not be rejected again"
    );
    Ok(())
}

/// WHEN the dispatch skips a terminal task and the queue holds another entry,
/// THE loop SHALL dispatch that next entry in the same idle cycle.
#[tokio::test]
async fn the_entry_behind_a_skipped_terminal_task_dispatches() -> Result<()> {
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
    // The worker is busy, so both arrivals wait in the queue: the terminal task
    // ahead of the live one.
    ctx_state.worker_active = true;
    let store = Arc::new(StagedStore::holding_terminal(&["task-a"]));
    enqueue(&bus, &[("task-a", "prompt a"), ("task-b", "prompt b")])?;

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
    let wire = tokio::spawn(run_wire(event_tx, worker_result_tx, worker_rx));

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
    let dispatched = wire.await.map_err(|_| "the wire task should not panic")??;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert_eq!(
        dispatched,
        Some("task-b".to_string()),
        "the skipped task must not hold the queue: the entry behind it dispatches in the same idle cycle"
    );
    assert!(
        store.rejections().is_empty(),
        "a skipped terminal task must not be rejected again"
    );
    Ok(())
}

/// WHEN a queued task's store state is not terminal at dispatch, THE loop SHALL
/// submit the turn with the task's attribution, exactly as before the fix.
#[tokio::test]
async fn a_queued_task_that_is_not_terminal_dispatches_with_its_attribution() -> Result<()> {
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
    // The worker is busy, so the arrival waits in the queue, and its store row is
    // still running when the dispatch reaches it.
    ctx_state.worker_active = true;
    let store = Arc::new(StagedStore::holding_terminal(&[]));
    enqueue(&bus, &[("task-a", "prompt a")])?;

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
    let wire = tokio::spawn(run_wire(event_tx, worker_result_tx, worker_rx));

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
    let dispatched = wire.await.map_err(|_| "the wire task should not panic")??;
    loop_result.map_err(|e| format!("the orchestrator loop should end cleanly: {e}"))?;
    assert_eq!(
        dispatched,
        Some("task-a".to_string()),
        "a task that is not terminal must run its attributed turn"
    );
    Ok(())
}

/// IF no external task store is configured, THEN THE loop SHALL dispatch queued
/// prompts exactly as before.
#[tokio::test]
async fn a_queued_task_dispatches_when_no_store_is_configured() -> Result<()> {
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
    // The worker is busy, so the arrival waits in the queue while no store is
    // configured to answer for it.
    ctx_state.worker_active = true;
    enqueue(&bus, &[("task-a", "prompt a")])?;

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
    let wire = tokio::spawn(run_wire(event_tx, worker_result_tx, worker_rx));

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
                external_task_store: None::<&Arc<StagedStore>>,
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
        "with no store configured the queue must dispatch its prompt as before"
    );
    Ok(())
}
