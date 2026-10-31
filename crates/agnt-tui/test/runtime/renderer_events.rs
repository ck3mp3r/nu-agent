use super::*;

use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::entry::TranscriptEntry;

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
    tui_active.emit(&SystemEvent::InferenceStarted);
    tui_active.emit(&SystemEvent::Tick);
    tui_active.emit(&SystemEvent::TurnCompleted {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });

    // -- Check: nothing is forwarded.
    assert!(events.lock().expect("events").is_empty());

    // -- Setup & Fixtures: non-TUI mode.
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut non_tui = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    // -- Exec
    non_tui.emit(&SystemEvent::SystemMessage {
        kind: SystemKind::Warning,
        text: "warn".to_string(),
    });

    // -- Check: the warning is forwarded.
    let captured = events.lock().expect("events").clone();
    assert_eq!(captured.len(), 1);
    assert!(matches!(
        captured[0],
        SystemEvent::SystemMessage {
            kind: SystemKind::Warning,
            ..
        }
    ));

    // -- Setup & Fixtures: TUI-active mode with payload-like events.
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut payload = TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    // -- Exec
    payload.emit(&SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("k8s__list_pods".to_string()),
            "mcp",
            r#"{"namespace":"prod"}"#,
            CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: r#"[{"name":"api-0"}]"#.to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    });
    payload.emit(&SystemEvent::InferenceChunk {
        delta: "response".to_string(),
        aggregated: "response".to_string(),
    });
    payload.emit(&SystemEvent::TurnCompleted {
        tool_calls: 1,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });

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
    coordinator.enqueue_event(SystemEvent::InferenceChunk {
        delta: "hello\nworld".to_string(),
        aggregated: "hello\nworld".to_string(),
    });
    coordinator.drain_transport();

    // -- Check: one assistant block with the raw markdown.
    let blocks = coordinator.state().transcript.entries();
    let assistant_entries: Vec<_> = blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            )
        })
        .collect();
    assert_eq!(assistant_entries.len(), 1);
    assert!(entry_text(assistant_entries[0]).contains("hello"));
    assert!(entry_text(assistant_entries[0]).contains("world"));

    // -- Setup & Fixtures: a markdown fixture.
    let mut markdown = RuntimeCoordinator::new(120, 30, Some(true));
    let fixture = markdown_fixture("lists_blockquote.md");

    // -- Exec
    markdown.enqueue_event(SystemEvent::InferenceChunk {
        delta: fixture.clone(),
        aggregated: fixture,
    });
    markdown.drain_transport();

    // -- Check: the projection produces list markers and quote rails.
    let raw_texts: Vec<String> = markdown
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_text)
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
        .enqueue_event(SystemEvent::InferenceChunk {
            delta: "world".to_string(),
            aggregated: "world".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    // -- Check: [User, Assistant] with no separator.
    assert_eq!(
        driver
            .state()
            .transcript
            .entries()
            .iter()
            .map(|block| (entry_to_role(block), entry_text(block)))
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
    driver.coordinator_mut().enqueue_event(SystemEvent::Tick);
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert!(driver.state().status.message.status_line().is_empty());

    // -- Exec
    driver
        .coordinator_mut()
        .enqueue_event(SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        });
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert!(driver.state().status.message.status_line().is_empty());

    // -- Setup & Fixtures: a busy turn.
    let mut busy = RenderLoopDriver::new(120, 30);

    // -- Exec
    busy.advance(&[key(TerminalKey::Char('k')), key(TerminalKey::Char('9'))])
        .await?;
    busy.coordinator_mut().state.phase = UiPhase::Busy;
    busy.coordinator_mut().enqueue_event(SystemEvent::Tick);
    busy.coordinator_mut().drain_transport();

    // -- Check: the status area reports busy.
    let status_lines =
        crate::runtime::status_lines_for_test(&mut busy.coordinator_mut().state, "openai/gpt-4");
    assert!(status_lines.join("\n").contains("(busy)"));

    // -- Exec
    busy.coordinator_mut()
        .enqueue_event(SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        });
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
