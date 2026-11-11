use super::*;

#[tokio::test]
async fn concurrent_in_flight_rules_allow_multiple_mcp_toggles() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        narration_rx: _,
        lifecycle_rx: _,
        stream_rx: _,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "gh".to_string(),
                enable: false,
            },
            &mut ctx,
        )
        .await;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::ToggleMcp {
                server: "fs".to_string(),
                enable: true,
            },
            &mut ctx,
        )
        .await;

    let mut mcp_toggles = 0;
    while let Some(cmd) = recv_command(worker_rx) {
        if matches!(
            cmd,
            WorkerCommand::HandleUiRequest {
                request: UiRequest::ToggleMcp { .. },
                ..
            }
        ) {
            mcp_toggles += 1;
        }
    }
    assert_eq!(
        mcp_toggles, 2,
        "multiple MCP toggles may be in-flight simultaneously"
    );

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "gh".to_string(),
                result: Ok(McpUsabilityState::Disabled),
                total: 0,
                server_count: 0,
                names_by_server: vec![],
            },
            &mut ctx,
        )
        .await;
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_concurrent_response(
            UiRequestResponse::McpToggle {
                server: "fs".to_string(),
                result: Ok(McpUsabilityState::Enabled),
                total: 0,
                server_count: 0,
                names_by_server: vec![],
            },
            &mut ctx,
        )
        .await;

    assert!(!stage.has_pending());
}

#[tokio::test]
async fn session_switch_success_rehydrates_transcript_from_snapshots() {
    let mut h = Harness::new();
    let HarnessParts {
        stage,
        worker_tx,
        blocking_tx,
        concurrent_tx,
        bus,
        worker_rx,
        narration_rx: _,
        lifecycle_rx: _,
        stream_rx,
        state,
    } = h.parts();
    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_incoming(
            UiRequest::SwitchSession {
                id: "sess-1".to_string(),
            },
            &mut ctx,
        )
        .await;
    let _ = recv_command(worker_rx);

    let mut ctx = make_ctx(worker_tx, blocking_tx, concurrent_tx, bus, state);
    stage
        .handle_blocking_response(
            UiRequestResponse::SessionSwitch {
                id: "sess-1".to_string(),
                result: Ok(vec![
                    UiMessageSnapshot::new("user", "first"),
                    UiMessageSnapshot::new("assistant", "second"),
                ]),
            },
            &mut ctx,
        )
        .await;

    // The switch marker and the replayed rows ride the stream: the surface
    // replaces its transcript on the hydrated marker, so there is no separate
    // clear or logo event to observe.
    let records = take_records(stream_rx);
    assert!(
        records
            .iter()
            .any(|r| matches!(r, SessionRecord::Session(SessionLifecycle::Switched { .. }))),
        "the switch must re-hydrate the transcript, got {records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, SessionRecord::Prompt(_)))
            .count(),
        1,
        "the replay carries one prompt row, got {records:?}"
    );
    assert!(
        records.iter().any(|r| matches!(
            r,
            SessionRecord::Inference(agnt_kernel::InferenceRecord::Chunk { aggregated, .. })
                if aggregated == "second"
        )),
        "the replay carries the assistant row, got {records:?}"
    );
}
