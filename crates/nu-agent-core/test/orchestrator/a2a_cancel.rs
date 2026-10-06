use super::*;

// ── CancellableBlockingRuntime ─────────────────────────────────────────
// Blocks in execute_turn until the block flag is unset OR a cancellation is
// requested via the ProgressUi. Used to verify that an A2A task cancel stops
// the running turn.

#[derive(Clone)]
struct CancellableBlockingRuntime {
    block: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    prompts: Arc<Mutex<Vec<String>>>,
    bus: crate::bus::Bus,
}

impl CancellableBlockingRuntime {
    fn new(block: Arc<AtomicBool>) -> Self {
        Self {
            block,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Arc::new(AtomicBool::new(false)),
            prompts: Arc::new(Mutex::new(Vec::new())),
            bus: crate::bus::Bus::default(),
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = bus;
        self
    }
}

impl CoreRuntime for CancellableBlockingRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.lock().expect("prompts lock").push(prompt);
        self.started.store(true, Ordering::SeqCst);
        // Subscribe to the bus cancel channel directly, matching production where
        // the hook/tool proxies subscribe to `bus.cancel()`.
        let mut cancel_rx = self.bus.cancel().subscribe();
        loop {
            tokio::select! {
                recv = cancel_rx.recv() => {
                    if matches!(recv, Ok(crate::bus::CancelEvent::Requested) | Err(crate::bus::ChannelError::Lagged { .. })) {
                        self.cancelled.store(true, Ordering::SeqCst);
                        let _ = self
                            .bus
                            .ui_event()
                            .send(UiEvent::Completed { tool_calls: 0 })
                            .await;
                        return Err(LabeledError::new("LLM call cancelled"));
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(2)) => {
                    if self.block.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for CancellableBlockingRuntime {
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

crate::default_session!(CancellableBlockingRuntime);
crate::default_mcp!(CancellableBlockingRuntime);

#[tokio::test]
async fn matching_a2a_task_cancel_sets_cancel_requested() -> TResult {
    let block = Arc::new(AtomicBool::new(false));
    let bus = create_bus();
    let runtime = CancellableBlockingRuntime::new(Arc::clone(&block)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_external_prompts(1)
        .with_bus(bus.clone());

    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Publish an external A2A task, retrying until the loop subscribes.
    let publish_bus = bus.clone();
    tokio::spawn(async move {
        let event = ExternalEvent::PromptReceived {
            prompt: "[A2A Task 11111111-2222-3333-4444-555555555555 from http://a.local]: do work"
                .to_string(),
            task_id: "11111111-2222-3333-4444-555555555555".to_string(),
            context_id: None,
        };
        while publish_bus.external().send(event.clone()).await.is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });

    // Send a matching cancel once the turn has started.
    let started = runtime.started.clone();
    let cancel_tx_clone = cancel_tx.clone();
    tokio::spawn(async move {
        while !started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        cancel_tx_clone
            .send("11111111-2222-3333-4444-555555555555".to_string())
            .unwrap();
    });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_task_cancel_rx(Some(cancel_rx))
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(
        runtime.cancelled(),
        "matching A2A task cancel must set cancel_requested and stop the turn"
    );
    // Unblock so the test exits cleanly.
    block.store(true, Ordering::SeqCst);
    Ok(())
}

#[tokio::test]
async fn non_matching_a2a_task_cancel_does_not_set_cancel_requested() -> TResult {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let bus = create_bus();
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_external_prompts(1)
        .with_bus(bus.clone());

    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Publish an external A2A task, retrying until the loop subscribes.
    let publish_bus = bus.clone();
    tokio::spawn(async move {
        let event = ExternalEvent::PromptReceived {
            prompt: "[A2A Task 11111111-2222-3333-4444-555555555555 from http://a.local]: do work"
                .to_string(),
            task_id: "11111111-2222-3333-4444-555555555555".to_string(),
            context_id: None,
        };
        while publish_bus.external().send(event.clone()).await.is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    cancel_tx
        .send("99999999-9999-9999-9999-999999999999".to_string())
        .unwrap();

    // Unblock the turn after a delay; a non-matching cancel must NOT cancel it.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_task_cancel_rx(Some(cancel_rx))
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    let prompts = runtime.prompts.lock().expect("prompts lock");
    assert_eq!(prompts.len(), 1, "one external prompt should be processed");
    assert!(
        prompts[0].starts_with("[A2A Task 11111111-2222-3333-4444-555555555555"),
        "non-matching cancel must not abort the running turn"
    );
    Ok(())
}

#[tokio::test]
async fn matching_a2a_task_cancel_stops_running_turn() -> TResult {
    let block = Arc::new(AtomicBool::new(false));
    let bus = create_bus();
    let runtime = CancellableBlockingRuntime::new(Arc::clone(&block)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_external_prompts(1)
        .with_bus(bus.clone());

    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Publish an external A2A task that blocks, then send a matching cancel
    // shortly after the turn has started.
    let publish_bus = bus.clone();
    tokio::spawn(async move {
        let event = ExternalEvent::PromptReceived {
            prompt: "[A2A Task 22222222-3333-4444-5555-666666666666 from http://a.local]: do work"
                .to_string(),
            task_id: "22222222-3333-4444-5555-666666666666".to_string(),
            context_id: None,
        };
        while publish_bus.external().send(event.clone()).await.is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });

    let started = runtime.started.clone();
    let cancel_tx_clone = cancel_tx.clone();
    tokio::spawn(async move {
        // Wait until the turn has started, then cancel the matching task.
        while !started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        cancel_tx_clone
            .send("22222222-3333-4444-5555-666666666666".to_string())
            .unwrap();
    });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_task_cancel_rx(Some(cancel_rx))
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(
        runtime.cancelled(),
        "matching A2A task cancel must stop the running turn"
    );
    // Unblock so the test exits cleanly.
    block.store(true, Ordering::SeqCst);
    Ok(())
}
