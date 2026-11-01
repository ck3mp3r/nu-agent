use super::*;

// ---------------------------------------------------------------------------
// Test: SwitchSession dispatches async load_session without panic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn switch_session_dispatches_async_load_without_panic()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let loaded_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut runtime = SwitchSessionRuntime {
        loaded_session_id: Arc::clone(&loaded_session_id),
        active_model: "openai/gpt-4o-mini".to_string(),
    };

    // result_tx is unused by SwitchSession but required by the dispatch signature
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);

    // Response channel for the SwitchSession command
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(1);

    let cmd = WorkerCommand::HandleUiRequest {
        request: UiRequest::SwitchSession {
            id: "test-session-123".to_string(),
        },
        response_tx,
    };

    // Dispatch from within an async context — this is the exact scenario
    // that was panicking with "Cannot start a runtime from within a runtime"
    // when load_session used Handle::current().block_on(...)
    let should_continue = CommandRouter::dispatch(
        cmd,
        &mut runtime,
        &result_tx,
        None::<NoAgentSwitchHook>,
        &create_bus(),
    )
    .await;

    // 1. No panic occurred (reaching here proves it)
    assert!(should_continue, "SwitchSession should not trigger shutdown");

    // 2. load_session was called with the correct session_id
    {
        let loaded = loaded_session_id.lock().expect("lock");
        assert_eq!(
            loaded.as_deref(),
            Some("test-session-123"),
            "load_session should be called with the correct session_id"
        );
    }

    // 3. The result is sent through the response channel
    let response = response_rx.recv().await.ok_or("response should be sent")?;
    match response {
        UiRequestResponse::SessionSwitch { id, result } => {
            assert_eq!(id, "test-session-123");
            assert!(
                result.is_err(),
                "default load_session should return an error"
            );
        }
        other => panic!("unexpected response: {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// on_agent_switch callback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn on_agent_switch_callback_invoked_after_successful_switch()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    let hook = RecordingAgentSwitchHook::default();
    let hook_calls = Arc::clone(&hook.calls);

    let bus = create_bus();
    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Ok("research-agent".to_string())),
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = AgentSwitchUi::new(&["research-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let config = InteractiveLoopConfig::new(Span::test_data())
        .with_on_agent_switch(hook)
        .with_bus(bus)
        .with_spawn_render_loop(spawner);

    // -- Exec
    let (_runtime, result) = run_interactive_loop_impl(runtime, config).await;

    // -- Check
    let value = result.map_err(|e| format!("interactive loop: {e:?}"))?;
    assert!(value.is_nothing());

    // Verify the hook was invoked with the correct values
    let calls = hook_calls.lock().expect("hook calls lock");
    assert_eq!(calls.len(), 1, "the hook must fire exactly once");
    assert_eq!(calls[0].0, "research-agent");
    // The AgentSwitchRuntime doesn't implement agent_description(), so it
    // returns None (the default trait method).
    assert_eq!(calls[0].1, None);
    // The AgentSwitchRuntime doesn't implement agent_icon(), so it
    // returns None (the default trait method).
    assert_eq!(calls[0].2, None);
    Ok(())
}
