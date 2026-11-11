use agnt_kernel::CompactionSource;

use super::*;

// ---------------------------------------------------------------------------
// Control ignore arms
// ---------------------------------------------------------------------------

/// A control command the loop does not own changes nothing: no turn, no enqueue,
/// no record, no spine row.
///
/// An owned command published after it on the same channel does act, which is
/// what makes the assertion sound: control delivers in publication order, so the
/// dispatched sweep proves the loop drained past the ignored command instead of
/// never reading it.
#[tokio::test]
async fn foreign_control_command_changes_nothing() -> Result<()> {
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
    let mut stream_rx = bus.stream().subscribe();
    let mut spine_rx = bus.lifecycle().subscribe();
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

    // Foreign: a panel request. The loop owns no action for it, so it ignores it
    // by name.
    bus.control()
        .publish(crate::bus::ControlCommand::SharedUiAction {
            action: crate::bus::SharedUiAction::Help,
        })
        .map_err(|_| "the control channel should be open")?;
    // Owned: a sweep request the loop dispatches to the worker.
    bus.control()
        .publish(crate::bus::ControlCommand::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .map_err(|_| "the control channel should be open")?;

    // No cancel source is configured, so the closed event channel is the loop's
    // exit: the owned command's sweep is the only thing it must reach first.
    let driver = async {
        match recv_worker_command(&mut worker_rx, "the owned command").await? {
            WorkerCommand::RunCompaction { source } => {
                assert_eq!(source, CompactionSource::Auto);
            }
            _ => return Err("the owned command must dispatch a RunCompaction".into()),
        }
        drop(event_tx);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, driven) = tokio::join!(
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
        ),
        driver
    );
    driven?;

    assert!(result.is_ok());
    // The ignored command dispatched nothing of its own.
    assert!(
        recv_command(&mut worker_rx).is_none(),
        "a panel request must dispatch no worker command"
    );
    // Nor did it submit a prompt, open a request, or start a turn.
    assert!(
        slash.handle_calls.is_empty(),
        "a panel request must not reach the slash stage: {:?}",
        slash.handle_calls
    );
    assert!(
        ui_request.handle_incoming_calls.is_empty(),
        "a panel request must not open a UI request"
    );
    assert!(
        !ctx_state.worker_active,
        "a panel request must not start a turn"
    );
    // Nor did it publish anything a consumer folds.
    assert!(
        stream_rx.try_recv().is_err(),
        "a panel request must publish no session record"
    );
    assert!(
        spine_rx.try_recv().is_err(),
        "a panel request must publish no spine row"
    );
    Ok(())
}
