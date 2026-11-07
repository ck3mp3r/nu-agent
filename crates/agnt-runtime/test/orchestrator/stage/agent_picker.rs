use super::*;

// ---------------------------------------------------------------------------
// Agent picker refresh
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_picker_refresh_dispatches_when_no_pending() -> Result<()> {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshAgentPicker, &mut ctx)
        .await;

    let cmd = recv_command(worker_rx).ok_or("RefreshAgentPicker should be dispatched")?;
    assert!(
        matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::RefreshAgentPicker,
                ..
            }
        ),
        "expected RefreshAgentPicker command when no refresh is pending"
    );
    Ok(())
}

#[tokio::test]
async fn agent_picker_refresh_in_flight_skips_new_command() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx: _,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshAgentPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshAgentPicker, &mut ctx)
        .await;

    assert!(
        recv_command(worker_rx).is_none(),
        "only one RefreshAgentPicker should be dispatched while one is pending"
    );
    assert!(stage.has_pending());
}

#[tokio::test]
async fn agent_picker_refresh_success_sets_options() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx: _,
        ui_state_rx,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshAgentPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::AgentRefresh(Ok(vec![persona_summary("planner")])),
            &mut ctx,
        )
        .await;

    let ui_events = take_ui_state(ui_state_rx);
    assert!(
        ui_events.iter().any(|e| {
            matches!(e, UiStateEvent::SetAgentPickerOptions(summaries)
                if summaries.len() == 1 && summaries[0].name == "planner")
        }),
        "agent picker options must be set from the scan"
    );
    assert!(!stage.has_pending());
}

#[tokio::test]
async fn agent_picker_refresh_error_warns() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        ui_event_rx,
        ui_state_rx: _,
        session_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(UiRequest::RefreshAgentPicker, &mut ctx)
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::AgentRefresh(Err("agent scan failed".to_string())),
            &mut ctx,
        )
        .await;

    let warnings = take_warnings(ui_event_rx);
    assert!(warnings.iter().any(|w| w == "agent scan failed"));
    assert!(!stage.has_pending());
}

/// One persona summary for the stage fixtures.
fn persona_summary(name: &str) -> crate::protocol::persona::PersonaSummary {
    crate::protocol::persona::PersonaSummary {
        name: name.to_string(),
        description: None,
        builtin: false,
    }
}
