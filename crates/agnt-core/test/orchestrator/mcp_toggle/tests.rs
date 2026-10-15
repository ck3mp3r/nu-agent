use super::*;

#[tokio::test]
async fn interactive_loop_processes_mcp_toggle_requests_and_updates_ui_state() -> TResult {
    let bus = create_bus();
    let runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Disabled,
        visible_count: 3,
        visible_count_by_server: 0,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: false,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), false)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![("gh".to_string(), McpUsabilityState::Disabled, None, 3)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 0)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_marks_enable_failure_as_failed_state() -> TResult {
    let bus = create_bus();
    let runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Failed,
        visible_count: 2,
        visible_count_by_server: 0,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: true,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![("gh".to_string(), McpUsabilityState::Failed, None, 2)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 0)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_marks_enable_success_as_enabled_state() -> TResult {
    let bus = create_bus();
    let runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Enabled,
        visible_count: 7,
        visible_count_by_server: 5,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: true,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![("gh".to_string(), McpUsabilityState::Enabled, None, 7)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 5)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_propagates_failure_reason_and_visible_tool_count_on_toggle_error()
-> TResult {
    let bus = create_bus();
    let runtime = FailingMcpToggleRuntime {
        toggles: Vec::new(),
        visible_count: 4,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: true,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![(
            "gh".to_string(),
            McpUsabilityState::Failed,
            Some("connect timeout".to_string()),
            4,
        )]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 0)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_toggle_enable_disable_cycle_refreshes_per_server_visible_counts() -> TResult {
    let bus = create_bus();
    let runtime = SequencedMcpToggleRuntime {
        toggles: Vec::new(),
        states: [McpUsabilityState::Disabled, McpUsabilityState::Enabled]
            .into_iter()
            .collect(),
        visible_counts: [3usize, 7usize].into_iter().collect(),
        visible_counts_by_server: [0usize, 5usize].into_iter().collect(),
        current_visible_count: 0,
        current_visible_count_by_server: 0,
        bus: bus.clone(),
    };
    let ui = StagedToggleUi::new().with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.toggles,
        vec![("gh".to_string(), false), ("gh".to_string(), true)]
    );
    Ok(())
}
