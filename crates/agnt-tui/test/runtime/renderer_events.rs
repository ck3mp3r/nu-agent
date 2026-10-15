use super::*;

/// The runtime renderer forwards events to the inner renderer only in non-TUI
/// mode; in TUI-active mode it swallows spinner progress and payload-like
/// events.
#[test]
fn runtime_renderer_forwarding_by_mode() -> Result<()> {
    // -- Setup & Fixtures: TUI-active mode.
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut tui_active = TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    // -- Exec
    tui_active.emit(&UiEvent::LlmStarted);
    tui_active.emit(&UiEvent::Tick);
    tui_active.emit(&UiEvent::Completed { tool_calls: 0 });

    // -- Check: nothing is forwarded.
    assert!(events.lock().expect("events").is_empty());

    // -- Setup & Fixtures: non-TUI mode.
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut non_tui = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    // -- Exec
    non_tui.emit(&UiEvent::Warning {
        message: "warn".to_string(),
    });

    // -- Check: the warning is forwarded.
    let captured = events.lock().expect("events").clone();
    assert_eq!(captured.len(), 1);
    assert!(matches!(captured[0], UiEvent::Warning { .. }));

    // -- Setup & Fixtures: TUI-active mode with payload-like events.
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut payload = TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    // -- Exec
    payload.emit(&UiEvent::ToolCompleted {
        name: "k8s__list_pods".to_string(),
        source: "mcp".to_string(),
        arguments: r#"{"namespace":"prod"}"#.to_string(),
        success: true,
        result: r#"[{"name":"api-0"}]"#.to_string(),
        display: None,
        error_kind: None,
        message: None,
    });
    payload.emit(&UiEvent::AssistantMessage {
        delta: "response".to_string(),
        text: "response".to_string(),
    });
    payload.emit(&UiEvent::Completed { tool_calls: 1 });

    // -- Check: nothing is forwarded.
    assert!(events.lock().expect("events").is_empty());
    Ok(())
}

/// An assistant message event is appended as one raw-markdown block, projects
/// its markdown, and flows after a user turn without a separator.
#[tokio::test]
async fn assistant_message_event_transcript_flow() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: "hello\nworld".to_string(),
        text: "hello\nworld".to_string(),
    });
    coordinator.drain_transport();

    // -- Check: one assistant block with the raw markdown.
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
    assert_eq!(assistant_entries.len(), 1);
    assert!(assistant_entries[0].source.plain_text().contains("hello"));
    assert!(assistant_entries[0].source.plain_text().contains("world"));

    // -- Setup & Fixtures: a markdown fixture.
    let mut markdown = RuntimeCoordinator::new(120, 30, Some(true));
    let fixture = markdown_fixture("lists_blockquote.md");

    // -- Exec
    markdown.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: fixture.clone(),
        text: fixture,
    });
    markdown.drain_transport();

    // -- Check: the projection produces list markers and quote rails.
    let raw_texts: Vec<String> = markdown
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();
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

    // -- Setup & Fixtures: a user turn then an assistant message.
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[key(TerminalKey::Char('h')), key(TerminalKey::Enter)])
        .await?;
    let _ = driver
        .coordinator_mut()
        .state
        .take_next_prompt_for_execution();
    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::AssistantMessage {
            delta: "world".to_string(),
            text: "world".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    // -- Check: [User, Assistant] with no separator.
    assert_eq!(
        driver
            .state()
            .transcript
            .blocks()
            .iter()
            .map(|block| (block_to_role(block), block.source.plain_text()))
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

/// Tick and Completed events update the status area only, without touching the
/// input buffer or polluting the input line.
#[tokio::test]
async fn tick_and_completed_events_update_status_only() -> Result<()> {
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

    // -- Setup & Fixtures: a busy turn.
    let mut busy = RenderLoopDriver::new(120, 30);

    // -- Exec
    busy.advance(&[key(TerminalKey::Char('k')), key(TerminalKey::Char('9'))])
        .await?;
    busy.coordinator_mut().state.phase = UiPhase::Busy;
    busy.coordinator_mut().enqueue_ui_event(UiEvent::Tick);
    busy.coordinator_mut().drain_transport();

    // -- Check: the status area reports busy.
    let status_lines =
        crate::runtime::status_lines_for_test(&mut busy.coordinator_mut().state, "openai/gpt-4");
    assert!(status_lines.join("\n").contains("(busy)"));

    // -- Exec
    busy.coordinator_mut()
        .enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    busy.coordinator_mut().drain_transport();

    // -- Check: the status area reports idle; the input line is not polluted.
    let status_lines =
        crate::runtime::status_lines_for_test(&mut busy.coordinator_mut().state, "openai/gpt-4");
    assert!(status_lines[0].contains("(idle)"));
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input mode:"))
    );
    Ok(())
}
