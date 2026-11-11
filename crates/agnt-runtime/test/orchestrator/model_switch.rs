use super::*;

#[tokio::test]
async fn palette_models_does_not_bypass_shared_models_action_path() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        vec![SharedUiAction::Models]
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn inline_model_picker_enter_switches_active_model_and_provider() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_min_bus_events(2)
        .with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.active_model_identity
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_failure_keeps_previous_model_and_warns() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        switch_model_result: Some(Err("switch failed".to_string())),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.active_model_identity
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        Some("openai/gpt-4o-mini".to_string())
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|w| w == "switch failed")
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_uses_cached_startup_plugin_config() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
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
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let _runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        *ui.active_model_identity
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_result_artifact_is_rendered() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let _runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert!(
        !ui.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|w| w.starts_with("Model switched"))
    );
    Ok(())
}

#[tokio::test]
async fn next_turn_uses_newly_selected_model() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["after-switch"]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
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
    let bus = Bus::default();
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

    // Unblock the turn once it has started and the fixture has injected every
    // queued prompt and model switch.
    let mut turn_started_rx = runtime.turn_started_rx();
    let mut drained_rx = ui.drained_rx();
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let _ = drained_rx.changed().await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let _runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .prompts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["first"]
    );
    assert_eq!(
        _runtime
            .switched_models
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["openai/gpt-4o-mini"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_applies_after_current_turn_before_next_dispatch() -> TResult {
    let bus = Bus::default();
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

    // Unblock the turn once it has started and the fixture has injected every
    // queued prompt and model switch.
    let mut turn_started_rx = runtime.turn_started_rx();
    let mut drained_rx = ui.drained_rx();
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let _ = drained_rx.changed().await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let _runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .action_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["turn:first", "turn:second", "switch:openai/gpt-4o-mini"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_last_write_wins() -> TResult {
    let bus = Bus::default();
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

    // Unblock the turn once it has started and the fixture has injected every
    // queued prompt and model switch.
    let mut turn_started_rx = runtime.turn_started_rx();
    let mut drained_rx = ui.drained_rx();
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let _ = drained_rx.changed().await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let _runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .switched_models
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["anthropic/claude-3-5-sonnet"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_failure_keeps_previous_model_and_warns() -> TResult {
    let bus = Bus::default();
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

    // Unblock the turn once it has started and the fixture has injected every
    // queued prompt and model switch.
    let mut turn_started_rx = runtime.turn_started_rx();
    let mut drained_rx = ui.drained_rx();
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let _ = drained_rx.changed().await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let _runtime = handed_back(_runtime)?;
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
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|w| w == "queued switch failed")
    );
    Ok(())
}
