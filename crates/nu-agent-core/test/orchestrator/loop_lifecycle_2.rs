use super::*;

// ── StartupHydrationRuntime ─────────────────────────────────────────────

struct StartupHydrationRuntime {
    names_by_server: Vec<(String, Vec<String>)>,
    bus: crate::bus::Bus,
}

impl CoreRuntime for StartupHydrationRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for StartupHydrationRuntime {
    async fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(McpUsabilityState::Disabled)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.names_by_server
            .iter()
            .map(|(_, names)| names.len())
            .sum()
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        self.names_by_server.clone()
    }
}

impl ModelSwitching for StartupHydrationRuntime {
    fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "unknown/unknown".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

crate::default_session!(StartupHydrationRuntime);

#[tokio::test]
async fn interactive_loop_processes_input_while_first_turn_is_running() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &[],
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
    let value = result.map_err(|e| format!("interactive loop should stay responsive: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second"]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_preserves_fifo_for_prompts_queued_while_active() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second", "third"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        3,
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
    result.map_err(|e| format!("interactive loop should complete queued prompts: {e}"))?;

    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second", "third"]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_global_abort_cancels_active_and_does_not_run_queued_prompt() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&["first"])
        .with_min_bus_events(2)
        .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task that unblocks the turn after a short delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (rt, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result
        .map_err(|e| format!("interactive loop should treat cancellation as non-fatal: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        rt.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_startup_hydration_initializes_per_server_visible_counts_before_toggles()
-> TResult {
    let bus = create_bus();
    let runtime = StartupHydrationRuntime {
        names_by_server: vec![
            (
                "gh".to_string(),
                vec!["gh__issues".to_string(), "gh__prs".to_string()],
            ),
            ("k8s".to_string(), vec!["k8s__pods".to_string()]),
        ],
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
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
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 2), ("k8s".to_string(), 1)]
    );
    Ok(())
}

#[test]
fn emit_batch_delivers_all_events() {
    // RED phase: write test that verifies emit_batch delivers all events
    struct BatchTestUi {
        event_tx: mpsc::Sender<OrchestratorEvent>,
        events: Vec<UiEvent>,
        emit_calls: usize,
        emit_batch_calls: usize,
    }

    impl Default for BatchTestUi {
        fn default() -> Self {
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
            Self {
                event_tx,
                events: Vec::new(),
                emit_calls: 0,
                emit_batch_calls: 0,
            }
        }
    }

    impl ProgressUi for BatchTestUi {
        fn emit(&mut self, event: &UiEvent) {
            self.events.push(event.clone());
            self.emit_calls += 1;
        }

        fn flush(&mut self) {}

        fn take_cancel_requested(&self) -> bool {
            false
        }

        fn emit_batch(&mut self, events: &[UiEvent]) {
            self.emit_batch_calls += 1;
            for event in events {
                self.events.push(event.clone());
            }
        }
    }

    impl UserInputUi for BatchTestUi {
        fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
            &self.event_tx
        }
    }

    let mut ui = BatchTestUi::default();

    // Create 5 test events
    let events = vec![
        UiEvent::Tick,
        UiEvent::LlmStarted,
        UiEvent::AssistantMessage {
            text: "hello".to_string(),
        },
        UiEvent::AssistantMessage {
            text: "world".to_string(),
        },
        UiEvent::Completed { tool_calls: 0 },
    ];

    // Call emit_batch
    ui.emit_batch(&events);

    // Verify all events were delivered
    assert_eq!(ui.events.len(), 5, "all 5 events should be delivered");
    assert_eq!(ui.emit_batch_calls, 1, "emit_batch should be called once");
    assert_eq!(
        ui.emit_calls, 0,
        "emit should not be called when using emit_batch"
    );
}
