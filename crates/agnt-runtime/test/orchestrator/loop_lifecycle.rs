use super::*;

use crate::session::tool_status::ToolStatus;

// ── ToolDisplayOnlyRuntime ──────────────────────────────────────────────

#[derive(Default)]
struct ToolDisplayOnlyRuntime {
    bus: crate::bus::Bus,
}

impl CoreRuntime for ToolDisplayOnlyRuntime {
    async fn execute_turn(
        &mut self,
        _prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        let call = ToolCall::new(
            ToolName("edit".to_string()),
            crate::tools::handler::ToolSource::Closure,
            "{}",
        );
        let _ = self
            .bus
            .stream()
            .publish(SessionRecord::Tool(ToolRecord::Started {
                call: call.clone(),
            }));
        let _ = self
            .bus
            .stream()
            .publish(SessionRecord::Tool(ToolRecord::Completed {
                call,
                outcome: ToolOutcome {
                    status: ToolStatus::Success,
                    payload: None,
                    text: r#"{"path":"file.txt","diff":"--- a/file.txt\n+++ b/file.txt\n"}"#
                        .to_string(),
                    message: None,
                    kind: None,
                },
            }));
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: None,
            duration_ms: 0,
        });
        TurnEnd::Completed {
            value: Value::nothing(span),
        }
    }
}

impl ModelSwitching for ToolDisplayOnlyRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
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
        prompt: String,
        _context: Option<String>,
        _span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        self.prompts.push(prompt);
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: None,
            duration_ms: 0,
        });
        if self.prompts.len() == 1 {
            return TurnEnd::Cancelled {
                value: Value::nothing(Span::test_data()),
            };
        }
        TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }
    }
}

impl ModelSwitching for CancelFirstRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
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
        prompt: String,
        _context: Option<String>,
        _span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        self.prompts.push(prompt);
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: None,
            duration_ms: 0,
        });
        if self.prompts.len() == 1 {
            return TurnEnd::Failed(TurnFailure::from_error(LabeledError::new(
                "API rate limit exceeded",
            )));
        }
        TurnEnd::Completed {
            value: Value::nothing(Span::test_data()),
        }
    }
}

impl ModelSwitching for ErrorFirstRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
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
    let bus = Bus::default();
    let mut runtime = ToolDisplayOnlyRuntime { bus: bus.clone() };
    let mut stream_rx = bus.stream().subscribe();

    let value = run_single_turn(
        &mut runtime,
        None,
        "show me diff".to_string(),
        None,
        Span::test_data(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    assert!(value.is_nothing());

    // Drain the stream (records published by the runtime).
    let mut events = Vec::new();
    loop {
        match stream_rx.try_recv() {
            Ok(event) => events.push(event),
            Err(crate::bus::TryReceiveError::Empty) => break,
            Err(crate::bus::TryReceiveError::Lagged(_)) => continue,
            Err(crate::bus::TryReceiveError::Closed) => break,
        }
    }

    assert!(
        events
            .iter()
            .any(|event| matches!(event, SessionRecord::Tool(ToolRecord::Completed { .. })))
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        SessionRecord::Inference(InferenceRecord::Chunk { .. })
    )));
    Ok(())
}

#[tokio::test]
async fn run_single_turn_uses_progress_ui_trait_boundary() -> TResult {
    let mut runtime = FakeRuntime::default();

    let value = run_single_turn(
        &mut runtime,
        None,
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
    let bus = Bus::default();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["a", "b"]).with_bus(bus.clone());

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
    assert_eq!(runtime.prompts, vec!["a".to_string(), "b".to_string()]);
    Ok(())
}

#[tokio::test]
async fn interactive_loop_does_not_return_per_turn_values_to_stdout() -> TResult {
    let bus = Bus::default();
    let runtime = FakeValueRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello"]).with_bus(bus.clone());

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
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn interactive_loop_treats_llm_cancellation_as_non_fatal_and_continues() -> TResult {
    let bus = Bus::default();
    let runtime = CancelFirstRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["first", "second"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
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
    let bus = Bus::default();
    let runtime = ErrorFirstRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["first", "second"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = drive_loop(
        runtime,
        LoopConfig::default().with_bus(bus).with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    let runtime = handed_back(runtime)?;
    let value = result.map_err(|e| format!("interactive loop should continue after error: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|w| w.contains("API rate limit exceeded")),
        "error should be displayed as inline warning"
    );
    Ok(())
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_before_first_pump() -> TResult {
    let bus = Bus::default();
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
    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default()
            .with_hydration(messages, None)
            .with_bus(bus)
            .with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    result.map_err(|e| format!("interactive loop with hydration: {e}"))?;

    assert_eq!(
        &ui.call_order.lock().unwrap_or_else(|e| e.into_inner())[..1],
        ["hydrate"],
        "expected hydrate before first pump"
    );
    Ok(())
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_exactly_once() -> TResult {
    let bus = Bus::default();
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
    let (_runtime, result) = drive_loop(
        runtime,
        LoopConfig::default()
            .with_hydration(messages, None)
            .with_bus(bus)
            .with_event_sink(spawner),
        Span::test_data(),
    )
    .await;
    result.map_err(|e| format!("interactive loop with hydration: {e}"))?;

    // The replay is a record log, not a snapshot list: the surface folds the
    // hydrated marker and then the projected rows, so exactly one marker opens
    // the log and the stored prompt comes back as a prompt record.
    let records = ui
        .stream_records
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert!(
        matches!(
            records.first(),
            Some(SessionRecord::Session(SessionLifecycle::Started {
                hydrated: true,
                ..
            }))
        ),
        "the replay must open with the hydrated marker, got {records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(
                r,
                SessionRecord::Session(SessionLifecycle::Started { hydrated: true, .. })
            ))
            .count(),
        1,
        "hydration must happen exactly once, got {records:?}"
    );
    assert!(
        records.iter().any(|r| matches!(
            r,
            SessionRecord::Prompt(prompt) if prompt.text == "history"
        )),
        "the replay must carry the stored prompt, got {records:?}"
    );
    Ok(())
}
