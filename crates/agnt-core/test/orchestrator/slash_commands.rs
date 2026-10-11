use super::*;

#[tokio::test]
async fn recognized_slash_commands_never_sent_to_llm() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/compact", "/skills",
    ])
    .with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn new_slash_command_clears_transcript_and_pushes_startup_logo() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/new"]).with_bus(bus.clone());

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
    assert_eq!(ui.clear_transcript_count.load(Ordering::SeqCst), 1);
    assert_eq!(ui.push_startup_logo_count.load(Ordering::SeqCst), 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn models_slash_command_not_sent_to_llm() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

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
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn models_slash_command_routes_to_shared_models_action() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_routes_compact_slash_to_compaction_executor() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact", "hello"])
        .with_min_bus_events(2)
        .with_expected_compaction_events(1)
        .with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn typed_compact_submit_triggers_compaction_path() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact"]).with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn interactive_loop_unknown_slash_emits_warning_and_continues() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui =
        FakeInteractiveUi::with_prompts(&["/compact now", "real prompt"]).with_bus(bus.clone());

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
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry == "Unknown slash command: /compact now")
    );
    assert_eq!(runtime.prompts, vec!["real prompt".to_string()]);
    Ok(())
}

#[tokio::test]
async fn recognized_slash_commands_not_persisted_as_session_turn_messages() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/compact"])
        .with_bus(bus.clone());

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
        runtime.run_compaction_calls, 1,
        "only /compact should route to session compaction"
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn manual_compaction_failure_is_not_surfaced_as_warning() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        run_compaction_fail: true,
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact"])
        .with_expected_compaction_events(0)
        .with_bus(bus.clone());

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
        runtime.run_compaction_calls, 1,
        "expected run_compaction to be invoked"
    );
    assert_eq!(
        runtime.run_compaction_sources,
        vec!["slash".to_string()],
        "expected /compact to dispatch RunCompaction with source 'slash'"
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .all(|w| { w != "auto compaction failed" }),
        "a failed compaction must not surface as a warning (it is fire-and-forget via the bus)"
    );
    Ok(())
}

#[tokio::test]
async fn slash_commands_reuse_command_palette_action_handlers() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/skills",
    ])
    .with_bus(bus.clone());

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
        ui.shared_actions.lock().unwrap().clone(),
        vec![
            SharedUiAction::Help,
            SharedUiAction::Status,
            SharedUiAction::Mcps,
            SharedUiAction::Models,
            SharedUiAction::Agents,
            SharedUiAction::Skills,
        ]
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn command_palette_models_action_opens_inline_model_picker() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"])
        .with_min_bus_events(2)
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    Ok(())
}

#[tokio::test]
async fn manual_compaction_slash_works_with_turn_processing() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello", "/compact"])
        .with_min_bus_events(2)
        .with_expected_compaction_events(1)
        .with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}
