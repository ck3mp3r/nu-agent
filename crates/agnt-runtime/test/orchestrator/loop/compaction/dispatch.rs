use agnt_kernel::CompactionSource;

use super::*;
use crate::protocol::events::SystemEvent;

// ---------------------------------------------------------------------------
// Compaction dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_compaction_dispatches_when_worker_idle() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_rx,
        worker_tx,
        mut worker_rx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
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

    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    let cmd = recv_command(&mut worker_rx).ok_or("RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, CompactionSource::Auto);
        }
        _ => panic!("expected RunCompaction"),
    }
    assert!(
        !ctx_state.worker_active,
        "worker_active must NOT be set for a compaction command (the worker emits events on the bus)"
    );
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_when_worker_busy_then_dispatched_on_idle() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    let mut session = MockSession::new();
    // Worker is busy running a turn.
    ctx_state.worker_active = true;
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .await
        .unwrap();
    // Worker finishes; the queued compaction must be dispatched.
    worker_result_tx
        .send(TurnResult::Success(Value::nothing(Span::test_data())))
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    let cmd = recv_command(&mut worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(source, CompactionSource::Auto);
        }
        _ => panic!("expected RunCompaction"),
    }
    Ok(())
}

#[tokio::test]
async fn run_compaction_queued_replaces_previous_when_busy() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_rx,
        worker_tx,
        mut worker_rx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
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

    // First request dispatches immediately and marks a compaction active.
    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .await
        .unwrap();
    // Second request queued while a compaction is active.
    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Manual,
        })
        .await
        .unwrap();
    // Third request replaces the queued one.
    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .await
        .unwrap();
    // The active compaction completes; the newest queued request is dispatched.
    bus.compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();
    // The dispatched queued compaction completes too, so the loop can exit.
    bus.compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    let cmd = recv_command(&mut worker_rx).ok_or("first RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Auto),
        _ => panic!("expected RunCompaction"),
    }
    let cmd = recv_command(&mut worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => {
            assert_eq!(
                source,
                CompactionSource::Auto,
                "newest queued request must win"
            );
        }
        _ => panic!("expected RunCompaction"),
    }
    Ok(())
}

#[tokio::test]
async fn compaction_requested_while_active_is_queued() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_rx,
        worker_tx,
        mut worker_rx,
        mut worker_result_rx,
        blocking_tx,
        mut blocking_response_rx,
        concurrent_tx,
        mut concurrent_response_rx,
        mut external_rx,
        mut compaction_rx,
        mut task_cancel_rx,
        bus,
        mut ctx_state,
        ..
    } = h;
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

    // First request dispatches immediately (worker idle, no compaction active).
    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .await
        .unwrap();
    // Second request arrives while a compaction is active — it must be queued,
    // not dispatched.
    bus.compaction()
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Manual,
        })
        .await
        .unwrap();
    // The active compaction completes; the queued request is dispatched.
    bus.compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "auto".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();
    // The dispatched queued compaction completes too, so the loop can exit.
    bus.compaction()
        .send(SystemEvent::CompactionCompleted {
            source: "slash".to_string(),
            summary_preview: String::new(),
            summary_body: String::new(),
        })
        .await
        .unwrap();

    let result = run_orchestrator_loop(
        event_rx,
        make_sources(
            &mut worker_result_rx,
            &mut blocking_response_rx,
            &mut concurrent_response_rx,
            &mut external_rx,
            &mut compaction_rx,
            &mut task_cancel_rx,
        ),
        Stages {
            slash: &mut slash,
            permission: &mut permission,
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    // First command: the initial request.
    let cmd = recv_command(&mut worker_rx).ok_or("first RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Auto),
        _ => panic!("expected RunCompaction"),
    }
    // Second command: the queued request dispatched after completion.
    let cmd = recv_command(&mut worker_rx).ok_or("queued RunCompaction should be dispatched")?;
    match cmd {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Manual),
        _ => panic!("expected RunCompaction"),
    }
    // No third command should be dispatched.
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "no further RunCompaction should be dispatched"
    );
    Ok(())
}
