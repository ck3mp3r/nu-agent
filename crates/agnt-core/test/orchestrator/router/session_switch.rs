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
    let should_continue =
        CommandRouter::dispatch(cmd, &mut runtime, &result_tx, None, &create_bus()).await;

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
    let switched_name = Arc::new(std::sync::Mutex::new(None::<String>));
    let switched_desc = Arc::new(std::sync::Mutex::new(None::<Option<String>>));
    let switched_icon = Arc::new(std::sync::Mutex::new(None::<Option<String>>));
    let cb_name = Arc::clone(&switched_name);
    let cb_desc = Arc::clone(&switched_desc);
    let cb_icon = Arc::clone(&switched_icon);

    let callback: OnAgentSwitch = Arc::new(
        move |name: String, description: Option<String>, icon: Option<String>| {
            *cb_name.lock().expect("name lock") = Some(name);
            *cb_desc.lock().expect("desc lock") = Some(description);
            *cb_icon.lock().expect("icon lock") = Some(icon);
        },
    );

    let bus = create_bus();
    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Ok("research-agent".to_string())),
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = AgentSwitchUi::new(&["research-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let config = InteractiveLoopConfig::new(Span::test_data())
        .with_on_agent_switch(callback)
        .with_bus(bus)
        .with_spawn_render_loop(spawner);

    let (_runtime, result) = run_interactive_loop_impl(runtime, config).await;
    let value = result.map_err(|e| format!("interactive loop: {e:?}"))?;
    assert!(value.is_nothing());

    // Verify the callback was invoked with the correct values
    let name = switched_name.lock().expect("name lock").take();
    let desc = switched_desc.lock().expect("desc lock").take();
    let icon = switched_icon.lock().expect("icon lock").take();
    assert_eq!(name.as_deref(), Some("research-agent"));
    // The AgentSwitchRuntime doesn't implement agent_description(), so it
    // returns None (the default trait method).
    assert_eq!(desc, Some(None));
    // The AgentSwitchRuntime doesn't implement agent_icon(), so it
    // returns None (the default trait method).
    assert_eq!(icon, Some(None));
    Ok(())
}
