use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Prompt routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prompt_submitted_routes_to_slash() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
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

    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await?;
    event_tx.send(OrchestratorEvent::Quit).await?;

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
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(slash.handle_calls, vec!["hello".to_string()]);
    Ok(())
}

/// A prompt that arrives while a blocking request is pending is deferred, not
/// submitted and not discarded.
#[tokio::test]
async fn prompt_submitted_while_blocking_pending_is_deferred() -> Result<()> {
    let h = Harness::new();
    let Harness {
        event_tx,
        event_rx,
        worker_tx,
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
    let mut slash = MockSlash::new();
    let mut permission = MockPermission::new();
    let mut ui_request = MockUiRequest::new();
    ui_request.blocking_pending = true;
    let mut session = MockSession::new();
    let mut ctx = make_ctx(
        &worker_tx,
        &blocking_tx,
        &concurrent_tx,
        &bus,
        &mut ctx_state,
    );

    event_tx
        .send(OrchestratorEvent::PromptSubmitted {
            text: "hello".to_string(),
        })
        .await?;
    event_tx.send(OrchestratorEvent::Quit).await?;

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
            ui_request: &mut ui_request,
            session: &mut session,
        },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
    assert!(
        slash.handle_calls.is_empty(),
        "slash.handle should NOT be called while the blocking request is pending"
    );
    Ok(())
}
