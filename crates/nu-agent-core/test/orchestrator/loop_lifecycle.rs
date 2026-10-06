use super::*;

// ── ToolDisplayOnlyRuntime ──────────────────────────────────────────────

#[derive(Default)]
struct ToolDisplayOnlyRuntime {
    bus: crate::bus::Bus,
}

impl CoreRuntime for ToolDisplayOnlyRuntime {
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
            .send(UiEvent::ToolStarted {
                name: "edit".to_string(),
                source: "closure".to_string(),
                arguments: "{}".to_string(),
                call_line: CallLine::from_json_summary("{}"),
            })
            .await;
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::ToolCompleted {
                name: "edit".to_string(),
                source: "closure".to_string(),
                arguments: "{}".to_string(),
                success: true,
                result: r#"{"path":"file.txt","diff":"--- a/file.txt\n+++ b/file.txt\n"}"#
                    .to_string(),
                display: Some(ToolDisplay {
                    title: "edit file.txt".to_string(),
                    sections: vec![ToolDisplaySection {
                        label: "file.txt".to_string(),
                        kind: ContentKind::Diff {
                            language: "diff".to_string(),
                        },
                        content: "--- a/file.txt\n+++ b/file.txt\n".to_string(),
                        stats: None,
                    }],
                }),
                error_kind: None,
                message: None,
            })
            .await;
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 1 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl ModelSwitching for ToolDisplayOnlyRuntime {
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

// ── CancelFirstRuntime ──────────────────────────────────────────────────

#[derive(Default)]
struct CancelFirstRuntime {
    prompts: Vec<String>,
    bus: crate::bus::Bus,
}

impl CoreRuntime for CancelFirstRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
            .await;
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("LLM call cancelled"));
        }
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for CancelFirstRuntime {
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

crate::default_session!(CancelFirstRuntime);
crate::default_mcp!(CancelFirstRuntime);

// ── ErrorFirstRuntime ───────────────────────────────────────────────────

#[derive(Default)]
struct ErrorFirstRuntime {
    prompts: Vec<String>,
    bus: crate::bus::Bus,
}

impl CoreRuntime for ErrorFirstRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
            .await;
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("API rate limit exceeded"));
        }
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for ErrorFirstRuntime {
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

crate::default_session!(ErrorFirstRuntime);
crate::default_mcp!(ErrorFirstRuntime);

#[tokio::test]
async fn tool_display_path_does_not_require_assistant_synthesis_round_trip() -> TResult {
    let bus = create_bus();
    let mut runtime = ToolDisplayOnlyRuntime { bus: bus.clone() };
    let mut ui_event_rx = bus.ui_event().subscribe();

    let value = run_single_turn(
        &mut runtime,
        &bus,
        "show me diff".to_string(),
        None,
        Span::test_data(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    assert!(value.is_nothing());

    // Drain the bus ui_event channel (events published by the runtime).
    let mut events = Vec::new();
    loop {
        match ui_event_rx.try_recv() {
            Ok(event) => events.push(event),
            Err(crate::bus::TryRecvError::Empty) => break,
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(crate::bus::TryRecvError::Closed) => break,
        }
    }

    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::ToolCompleted {
            display: Some(_),
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, UiEvent::AssistantMessage { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn run_single_turn_uses_progress_ui_trait_boundary() -> TResult {
    let bus = create_bus();
    let mut runtime = FakeRuntime::default();

    let value = run_single_turn(
        &mut runtime,
        &bus,
        "hello".to_string(),
        Some("ctx".to_string()),
        Span::test_data(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn run_interactive_loop_uses_interactive_ui_trait_boundary() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["a", "b"]).with_bus(bus.clone());

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
    assert_eq!(runtime.prompts, vec!["a".to_string(), "b".to_string()]);
    Ok(())
}

#[tokio::test]
async fn interactive_loop_does_not_return_per_turn_values_to_stdout() -> TResult {
    let bus = create_bus();
    let runtime = FakeValueRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello"]).with_bus(bus.clone());

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
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn interactive_loop_treats_llm_cancellation_as_non_fatal_and_continues() -> TResult {
    let bus = create_bus();
    let runtime = CancelFirstRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["first", "second"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value =
        result.map_err(|e| format!("interactive loop should continue after cancellation: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_treats_errors_as_non_fatal_and_displays_inline() -> TResult {
    let bus = create_bus();
    let runtime = ErrorFirstRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["first", "second"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop should continue after error: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.contains("API rate limit exceeded")),
        "error should be displayed as inline warning"
    );
    Ok(())
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_before_first_pump() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let messages = vec![
        UiMessageSnapshot::new("user", "from history"),
        UiMessageSnapshot::new("assistant", "from assistant"),
    ];

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_hydration(messages, None)
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop with hydration: {e}"))?;

    assert_eq!(
        &ui.call_order.lock().expect("call_order lock")[..1],
        ["hydrate"],
        "expected hydrate before first pump"
    );
    Ok(())
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_exactly_once() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let messages = vec![UiMessageSnapshot::new("user", "history"), {
        let mut s = UiMessageSnapshot::new("assistant", "response");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(321),
        });
        s
    }];
    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_hydration(messages.clone(), None)
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop with hydration: {e}"))?;

    assert_eq!(
        *ui.hydrated_messages.lock().expect("hydrated_messages lock"),
        messages
    );
    Ok(())
}
