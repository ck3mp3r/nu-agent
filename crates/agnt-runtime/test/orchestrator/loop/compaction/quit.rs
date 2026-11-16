//! Quit gating around a transcript rewrite.
//!
//! The loop derives the active rewrite from the spine, so these tests make one
//! active the way the pipeline does: with a `CompactionStarted` row. Quit does
//! not defer on it — it asks the sweep to stop, waits one window for the sweep's
//! own terminal row, and releases what never answers.

use agnt_kernel::bus::{CancelEvent, LifecycleEvent};
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, SessionRecord};

use super::*;

// ---------------------------------------------------------------------------
// The cancel answers
// ---------------------------------------------------------------------------

/// WHEN a sweep is active and quit arrives, THE QuitArm SHALL request the
/// cancel; WHEN the engine's cancel arm answers with its terminal row, THE loop
/// SHALL exit on that row and publish no row of its own.
#[tokio::test]
async fn quit_cancels_the_active_sweep_and_exits_on_its_row() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_result_tx: _,
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
    let bus_driver = bus.clone();
    let mut cancel_rx = bus.cancel().subscribe();
    let mut stream_rx = bus.stream().subscribe();

    let loop_task = tokio::spawn(async move {
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
        run_orchestrator_loop(
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
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    // The pipeline says a sweep began — the only thing that makes one active.
    bus_driver
        .lifecycle()
        .publish(LifecycleEvent::CompactionStarted {
            source: CompactionSource::Auto,
        })?;

    // -- Exec
    event_tx.send(OrchestratorEvent::Quit).await?;
    let asked = tokio::time::timeout(std::time::Duration::from_secs(5), cancel_rx.recv())
        .await
        .map_err(|_| "quit should ask the sweep to stop")?;
    assert!(
        matches!(asked, Ok(CancelEvent::Requested)),
        "quit requests the cancel the sweep answers"
    );
    // The engine's cancel arm answers with its own terminal row.
    bus_driver
        .lifecycle()
        .publish(LifecycleEvent::CompactionFailed {
            source: CompactionSource::Auto,
            message: "Compaction cancelled by user".to_string(),
        })?;

    // -- Check: the loop exits on that row, and publishes none of its own.
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok());
    let mut released = Vec::new();
    while let Ok(record) = stream_rx.try_recv() {
        if let SessionRecord::Compaction(CompactionRecord::Failed { message, .. }) = record {
            released.push(message);
        }
    }
    assert!(
        released.is_empty(),
        "a sweep that answered the cancel needs no row from quit; got {released:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The sweep never answers
// ---------------------------------------------------------------------------

/// WHEN the sweep publishes no terminal row within the cancel window, THEN THE
/// QuitArm SHALL publish the sweep's terminal rows itself and the loop SHALL
/// exit.
#[tokio::test(start_paused = true)]
async fn quit_releases_a_sweep_that_never_answers() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        worker_result_tx: _,
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
    let bus_driver = bus.clone();
    let mut stream_rx = bus.stream().subscribe();

    let loop_task = tokio::spawn(async move {
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
        run_orchestrator_loop(
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
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    bus_driver
        .lifecycle()
        .publish(LifecycleEvent::CompactionStarted {
            source: CompactionSource::Manual,
        })?;

    // -- Exec: no terminal row ever follows the cancel.
    event_tx.send(OrchestratorEvent::Quit).await?;

    // -- Check: quit waits its window, publishes both rows, and exits.
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(result.is_ok(), "the loop exits on a sweep it released");
    let mut released = Vec::new();
    while let Ok(record) = stream_rx.try_recv() {
        if let SessionRecord::Compaction(CompactionRecord::Failed { source, message }) = record {
            released.push((source, message));
        }
    }
    assert_eq!(
        released,
        vec![(
            CompactionSource::Manual,
            "Compaction abandoned at quit".to_string()
        )],
        "the sweep quit released ends with its own terminal row, once"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The queued rewrite
// ---------------------------------------------------------------------------

/// WHEN a compaction is queued when quit arrives, THEN THE QuitArm SHALL publish
/// its terminal rows and release it, so the loop is owed nothing.
#[tokio::test]
async fn quit_releases_the_queued_compaction() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
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
    let bus_driver = bus.clone();
    let mut stream_rx = bus.stream().subscribe();

    let loop_task = tokio::spawn(async move {
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
        run_orchestrator_loop(
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
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    // The worker takes the first request and the pipeline says its sweep
    // began; the second request then parks in the slot quit must release.
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Manual,
        })?;
    match recv_worker_command(&mut worker_rx, "RunCompaction").await? {
        WorkerCommand::RunCompaction { source } => assert_eq!(source, CompactionSource::Manual),
        _ => panic!("expected RunCompaction"),
    }
    bus_driver
        .lifecycle()
        .publish(LifecycleEvent::CompactionStarted {
            source: CompactionSource::Manual,
        })?;
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Auto,
        })?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "the second request parks while the first runs"
    );

    // -- Exec
    event_tx.send(OrchestratorEvent::Quit).await?;

    // -- Check
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(
        result.is_ok(),
        "a released rewrite gives the loop nothing to wait for"
    );
    let mut released = Vec::new();
    while let Ok(record) = stream_rx.try_recv() {
        if let SessionRecord::Compaction(CompactionRecord::Failed { source, message }) = record {
            released.push((source, message));
        }
    }
    assert_eq!(
        released,
        vec![
            (
                CompactionSource::Manual,
                "Compaction abandoned at quit".to_string()
            ),
            (
                CompactionSource::Auto,
                "Compaction abandoned at quit".to_string()
            ),
        ],
        "the sweep is released first, then the parked request, once each"
    );
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a released request is never dispatched after the quit"
    );
    Ok(())
}

/// WHEN an uncompaction is queued when quit arrives, THEN THE QuitArm SHALL
/// publish the warning a refusal draws and the distinct `UncompactFailed` spine
/// row, once each — an undo the user asked for never ends in silence either.
#[tokio::test]
async fn quit_releases_the_queued_uncompaction() -> Result<()> {
    // -- Setup & Fixtures
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
        mut worker_rx,
        worker_result_tx: _,
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
    let bus_driver = bus.clone();
    let mut stream_rx = bus.stream().subscribe();
    let mut spine_rx = bus.lifecycle().subscribe();

    let loop_task = tokio::spawn(async move {
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
        run_orchestrator_loop(
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
                ui_request: &mut ui_request,
                session: &mut session,
            },
            &mut ctx,
        )
        .await
    });

    // A sweep is active, so the undo parks in the queue quit must release.
    bus_driver
        .lifecycle()
        .publish(LifecycleEvent::CompactionStarted {
            source: CompactionSource::Auto,
        })?;
    bus_driver
        .control()
        .publish(crate::bus::ControlCommand::UncompactRequested {
            source: CompactionSource::Manual,
        })?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "the undo parks while a rewrite is active"
    );

    // -- Exec
    event_tx.send(OrchestratorEvent::Quit).await?;

    // -- Check: the undo's own rows, once each, then the loop exits.
    let result = loop_task
        .await
        .map_err(|e| format!("loop task should not panic: {e}"))?;
    assert!(
        result.is_ok(),
        "a released undo gives the loop nothing to wait for"
    );
    let mut notices = Vec::new();
    while let Ok(record) = stream_rx.try_recv() {
        if let SessionRecord::Notice(notice) = record {
            notices.push((notice.kind, notice.text));
        }
    }
    assert_eq!(
        notices,
        vec![(
            NoticeKind::Warning,
            "Nothing undone: Uncompaction abandoned at quit".to_string()
        )],
        "the released undo draws the refusal a warning draws, once"
    );
    let mut released = Vec::new();
    while let Ok(event) = spine_rx.try_recv() {
        if let LifecycleEvent::UncompactFailed { source, message } = event {
            released.push((source, message));
        }
    }
    assert_eq!(
        released,
        vec![(
            CompactionSource::Manual,
            "Uncompaction abandoned at quit".to_string()
        )],
        "the released undo ends with its own spine row, once"
    );
    Ok(())
}
