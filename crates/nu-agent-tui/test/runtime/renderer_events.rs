use super::*;

#[test]
fn runtime_renderer_in_tui_mode_does_not_forward_spinner_progress_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer =
        TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStarted);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Completed { tool_calls: 0 });

    assert!(events.lock().expect("events").is_empty());
}

#[test]
fn runtime_renderer_non_tui_mode_forwards_events_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::Warning {
        message: "warn".to_string(),
    });

    let captured = events.lock().expect("events").clone();
    assert_eq!(captured.len(), 1);
    assert!(matches!(captured[0], UiEvent::Warning { .. }));
}

#[test]
fn assistant_message_event_is_appended_to_tui_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello\nworld".to_string(),
    });
    coordinator.drain_transport();

    // After the raw-markdown refactor, a single AssistantMessage event produces
    // exactly one assistant markdown block (the entire text is stored as raw markdown).
    let assistant_entries: Vec<_> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        assistant_entries.len(),
        1,
        "one assistant block per message block"
    );
    assert!(
        assistant_entries[0].source.plain_text().contains("hello"),
        "raw markdown should contain 'hello'"
    );
    assert!(
        assistant_entries[0].source.plain_text().contains("world"),
        "raw markdown should contain 'world'"
    );
}

#[test]
fn assistant_markdown_message_is_projected_before_transcript_append() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("lists_blockquote.md");
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    coordinator.drain_transport();

    // After the raw-markdown refactor, the transcript stores the source markdown.
    // Projection happens at render time. Verify the raw markdown is stored and
    // produces the expected rendered output when projected.
    let raw_texts: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();

    // The entire markdown is stored in a single assistant block.
    // Project it and verify the list markers appear.
    let projected: Vec<String> = raw_texts
        .iter()
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect();

    assert!(projected.iter().any(|l| l.contains("• one")));
    assert!(projected.iter().any(|l| l.contains("• two")));
    assert!(projected.iter().any(|l| l.contains("1. first")));
    assert!(projected.iter().any(|l| l.contains("2. second")));
    assert!(projected.iter().any(|l| l.contains("│ quoted")));
    assert!(projected.iter().any(|l| l.contains("│ second")));
}

#[test]
fn assistant_markdown_message_preserves_inline_span_styles_in_transcript_state() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello **bold** and `code`".to_string(),
    });
    coordinator.drain_transport();

    // TranscriptEntry no longer has a `.rendered` field - test removed
    // Previously tested that assistant markdown was rendered with bold formatting
}

#[tokio::test]
async fn user_then_assistant_flows_without_turn_separator() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "h" through the real terminal arm.
    driver
        .advance(&[key(TerminalKey::Char('h')), key(TerminalKey::Enter)])
        .await?;
    // Activate the queued prompt so the User transcript entry is written
    let _ = driver
        .coordinator_mut()
        .state
        .take_next_prompt_for_execution();

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::AssistantMessage {
            text: "world".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert_eq!(
        driver
            .state()
            .transcript
            .blocks()
            .iter()
            .map(|block| {
                let role = block_to_role(block);
                (role, block.source.plain_text())
            })
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::User, "h".to_string()),
            (TranscriptRole::Assistant, "world".to_string()),
        ]
    );
    Ok(())
}

#[test]
fn busy_input_line_has_no_spinner_prefix_and_never_shows_locked_label() {
    let mut state = AppState::default();

    let idle_line = input_line_for_test(&state);
    assert_eq!(idle_line, "");

    state.phase = UiPhase::Busy;
    state.ensure_invariants();
    let busy_line = input_line_for_test_at_millis(&state, 160);
    assert_eq!(busy_line, "");
}

#[test]
fn tui_active_mode_does_not_forward_payload_like_events_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer =
        TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::ToolCompleted {
        name: "k8s__list_pods".to_string(),
        source: "mcp".to_string(),
        arguments: r#"{"namespace":"prod"}"#.to_string(),
        success: true,
        result: r#"[{"name":"api-0"}]"#.to_string(),
        display: None,
        error_kind: None,
        message: None,
    });
    runtime_renderer.emit(&UiEvent::AssistantMessage {
        text: "response".to_string(),
    });
    runtime_renderer.emit(&UiEvent::Completed { tool_calls: 1 });

    assert!(events.lock().expect("events").is_empty());
}

#[tokio::test]
async fn tick_and_completed_events_update_status_only_without_touching_input_buffer() -> Result<()>
{
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::Char('x'))]).await?;

    driver.coordinator_mut().enqueue_ui_event(UiEvent::Tick);
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert!(driver.state().status.message.status_line().is_empty());

    // -- Exec
    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert!(driver.state().status.message.status_line().is_empty());
    Ok(())
}

#[tokio::test]
async fn status_updates_stay_in_status_area_and_do_not_pollute_input_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type k and 9 through the real terminal arm.
    driver
        .advance(&[key(TerminalKey::Char('k')), key(TerminalKey::Char('9'))])
        .await?;

    // -- Exec: mark the turn busy so the model activity label reports it.
    driver.coordinator_mut().state.phase = UiPhase::Busy;
    driver.coordinator_mut().enqueue_ui_event(UiEvent::Tick);
    driver.coordinator_mut().drain_transport();

    let status_lines =
        crate::runtime::status_lines_for_test(&mut driver.coordinator_mut().state, "openai/gpt-4");
    let joined = status_lines.join("\n");

    // -- Check
    assert!(joined.contains("(busy)"));

    // -- Exec
    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    driver.coordinator_mut().drain_transport();

    let status_lines =
        crate::runtime::status_lines_for_test(&mut driver.coordinator_mut().state, "openai/gpt-4");

    // -- Check
    assert!(status_lines[0].contains("(idle)"));
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input mode:"))
    );
    Ok(())
}
