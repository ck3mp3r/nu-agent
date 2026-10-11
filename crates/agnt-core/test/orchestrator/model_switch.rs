use super::*;

#[tokio::test]
async fn palette_models_does_not_bypass_shared_models_action_path() -> TResult {
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
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn inline_model_picker_enter_switches_active_model_and_provider() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_min_bus_events(2)
        .with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

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
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.active_model_identity.lock().unwrap(),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_failure_keeps_previous_model_and_warns() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        switch_model_result: Some(Err("switch failed".to_string())),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

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
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.active_model_identity.lock().unwrap(),
        Some("openai/gpt-4o-mini".to_string())
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w == "switch failed")
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_uses_cached_startup_plugin_config() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

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
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_updates_footer_active_model_identity_immediately() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

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
        *ui.active_model_identity.lock().unwrap(),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_result_artifact_is_rendered() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

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
    assert!(
        !ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.starts_with("Model switched"))
    );
    Ok(())
}

#[tokio::test]
async fn next_turn_uses_newly_selected_model() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["after-switch"]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

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
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(runtime.prompts, vec!["after-switch".to_string()]);
    Ok(())
}

#[tokio::test]
async fn model_switch_while_worker_active_is_queued_for_next_turn() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

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
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    assert_eq!(
        _runtime
            .switched_models
            .lock()
            .expect("switched models lock")
            .as_slice(),
        ["openai/gpt-4o-mini"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_applies_after_current_turn_before_next_dispatch() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

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
        _runtime
            .action_log
            .lock()
            .expect("action log lock")
            .as_slice(),
        ["turn:first", "turn:second", "switch:openai/gpt-4o-mini"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_last_write_wins() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini", "anthropic/claude-3-5-sonnet"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

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
        _runtime
            .switched_models
            .lock()
            .expect("switched models lock")
            .as_slice(),
        ["anthropic/claude-3-5-sonnet"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_failure_keeps_previous_model_and_warns() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn))
        .with_switch_model_result(Err("queued switch failed".to_string()))
        .with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

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
        _runtime.active_model_identity(),
        "openai/gpt-4o-mini",
        "failed queued switch must keep previous active identity"
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w == "queued switch failed")
    );
    Ok(())
}
