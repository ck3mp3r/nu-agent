use super::*;

#[tokio::test]
async fn recognized_slash_commands_never_sent_to_llm() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/compact", "/skills",
    ])
    .with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn new_slash_command_clears_transcript_and_pushes_startup_logo() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/new"]).with_bus(bus.clone());

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
    assert!(runtime.prompts.is_empty());
    assert!(
        ui.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty(),
        "/new while the worker is idle applies at once and warns nothing"
    );
    Ok(())
}

/// WHILE a turn runs, WHEN `/new` arrives, THE SlashStage SHALL send the mint
/// command AND publish exactly one warning naming the deferral.
///
/// The mint takes effect only when the turn ends, and it used to defer in
/// silence — the "command did not fire" complaint.
#[tokio::test]
async fn new_slash_command_warns_once_while_turn_busy() -> TResult {
    let bus = Bus::default();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["/new"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Unblock the turn once it has started and the fixture has injected the
    // queued `/new`.
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
    let runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    // The deferred mint never ran a turn of its own.
    assert_eq!(
        runtime
            .prompts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["first"]
    );
    let warnings = ui
        .warnings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert_eq!(
        warnings,
        vec!["/new applies after the current turn finishes"],
        "a deferred /new warns exactly once"
    );
    Ok(())
}

/// WHEN a model switch, `/new`, and an agent switch all defer behind a blocked
/// turn, THE loop SHALL apply both switches once that turn ends.
///
/// The first switch applies on the turn-end drain. The second one used to wait
/// for the NEXT turn end — which never comes while the session sits idle — so a
/// warned switch was silently dropped. The blocking-response arm drains too, so
/// each applied switch releases the slot for the next queued one.
#[tokio::test]
async fn test_orchestrator_loop_switch_new_switch_applies_both() -> TResult {
    // -- Setup & Fixtures: a blocked turn, and the surface's own event sender so
    // the three inputs can be injected in order while the turn runs.
    let bus = Bus::default();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&["first"]).with_bus(bus.clone());

    let inject_tx: Arc<Mutex<Option<mpsc::Sender<OrchestratorEvent>>>> = Arc::new(Mutex::new(None));
    let sink = ui.make_event_spawner();
    let captured = Arc::clone(&inject_tx);
    let spawner = move |event_tx: mpsc::Sender<OrchestratorEvent>| {
        *captured.lock().unwrap_or_else(|e| e.into_inner()) = Some(event_tx.clone());
        sink(event_tx);
    };

    let mut turn_started_rx = runtime.turn_started_rx();
    let unblock = Arc::clone(&block_first_turn);
    let injector = tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let Some(event_tx) = inject_tx.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
            return;
        };
        let _ = event_tx
            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                spec: "m".to_string(),
            }))
            .await;
        let _ = event_tx
            .send(OrchestratorEvent::PromptSubmitted {
                text: "/new".to_string(),
            })
            .await;
        let _ = event_tx
            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                name: "a".to_string(),
            }))
            .await;
        // Every deferral is queued before the turn ends, so the end-of-turn
        // drain is the only thing that can apply them.
        unblock.store(true, Ordering::SeqCst);
    });

    // -- Exec
    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(_runtime)?;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;
    injector.await?;

    // -- Check: both switches applied after the one turn, in queue order, and
    // each deferral said so exactly once.
    assert!(value.is_nothing());
    assert_eq!(
        runtime
            .prompts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["first"],
        "no switch runs a turn of its own"
    );
    assert_eq!(
        runtime
            .switched_models
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["m"],
        "the queued model switch applies"
    );
    assert_eq!(
        runtime
            .switched_agents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["a"],
        "the second queued switch applies without another turn"
    );
    let warnings = ui
        .warnings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert_eq!(
        warnings,
        vec![
            "model switch applies after the current turn finishes",
            "/new applies after the current turn finishes",
            "agent switch applies after the current turn finishes",
        ],
        "each deferred effect warns exactly once, in arrival order"
    );
    Ok(())
}

#[tokio::test]
async fn models_slash_command_not_sent_to_llm() -> TResult {
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
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn models_slash_command_routes_to_shared_models_action() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

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
        ui.shared_actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        vec![SharedUiAction::Models]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_routes_compact_slash_to_compaction_executor() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact", "hello"])
        .with_min_bus_events(2)
        .with_expected_compaction_events(1)
        .with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn typed_compact_submit_triggers_compaction_path() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact"]).with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn interactive_loop_unknown_slash_emits_warning_and_continues() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui =
        FakeInteractiveUi::with_prompts(&["/compact now", "real prompt"]).with_bus(bus.clone());

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
    assert!(
        ui.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|entry| entry == "Unknown slash command: /compact now")
    );
    assert_eq!(runtime.prompts, vec!["real prompt".to_string()]);
    Ok(())
}

#[tokio::test]
async fn recognized_slash_commands_not_persisted_as_session_turn_messages() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/compact"])
        .with_bus(bus.clone());

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
        runtime.run_compaction_calls, 1,
        "only /compact should route to session compaction"
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn manual_compaction_failure_is_not_surfaced_as_warning() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        run_compaction_fail: true,
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact"])
        .with_expected_compaction_events(0)
        .with_bus(bus.clone());

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
        runtime.run_compaction_calls, 1,
        "expected run_compaction to be invoked"
    );
    assert_eq!(
        runtime.run_compaction_sources,
        vec![CompactionSource::Manual],
        "expected /compact to dispatch RunCompaction with the manual source"
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .all(|w| { w != "auto compaction failed" }),
        "a failed compaction must not surface as a warning (it is fire-and-forget via the bus)"
    );
    Ok(())
}

#[tokio::test]
async fn slash_commands_reuse_command_palette_action_handlers() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/skills",
    ])
    .with_bus(bus.clone());

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
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"])
        .with_min_bus_events(2)
        .with_bus(bus.clone());

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
        ui.shared_actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        vec![SharedUiAction::Models]
    );
    Ok(())
}

#[tokio::test]
async fn manual_compaction_slash_works_with_turn_processing() -> TResult {
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello", "/compact"])
        .with_min_bus_events(2)
        .with_expected_compaction_events(1)
        .with_bus(bus.clone());

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
    assert_eq!(runtime.run_compaction_calls, 1);
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}
