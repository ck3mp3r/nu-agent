use super::*;

use agnt_core::transcript::entry::TranscriptEntry;

/// Idle startup does not show a spinner; a submit handoff keeps input editable
/// and preserves the transcript preview.
#[tokio::test]
async fn idle_startup_and_submit_handoff() -> Result<()> {
    // -- Setup & Fixtures
    let coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec & Check: idle startup.
    assert_ne!(
        coordinator.state().status.message.status_line(),
        "Thinking..."
    );
    assert!(!coordinator.state().input_locked);
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("○ "));

    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;

    // -- Check: busy, input editable, prompt handed off, transcript preview.
    assert_eq!(driver.state().phase, UiPhase::Busy);
    assert!(!driver.state().input_locked);
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "x")
        )
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert_eq!(driver.state().transcript.len(), 1);
    assert!(matches!(
        &driver.state().transcript.entries()[0],
        TranscriptEntry::Message(message) if message.role == MessageRole::User
    ));
    assert_eq!(entry_text(&driver.state().transcript.entries()[0]), "x");
    Ok(())
}

/// Slash commands do not append command text to the transcript, the compact
/// result artifact is visible without a slash echo, and a slash prefix filters
/// suggestions and submits the matching command.
#[tokio::test]
async fn slash_commands_handoff() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.coordinator_mut().textarea = ratatui_textarea::TextArea::new(vec!["/help".to_string()]);

    // -- Exec
    driver.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check: the command is handed off; no transcript echo.
    assert!(driver.orchestrator_events().iter().any(
        |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "/help")
    ));
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    assert_eq!(driver.state().pending_prompt_count(), 0);
    assert!(driver.state().prompt_items().is_empty());
    assert!(driver.state().transcript.entries().is_empty());

    // -- Setup & Fixtures: /compact.
    let mut compact = RenderLoopDriver::new(120, 30);
    let script: Vec<DriveEvent> = "/compact"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();

    // -- Exec
    compact.advance(&script).await?;

    // -- Check: the command is handed off; no transcript echo.
    assert!(compact.orchestrator_events().iter().any(
        |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "/compact")
    ));
    assert_eq!(
        compact
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert!(compact.state().transcript.entries().is_empty());

    // -- Exec: the compaction artifact.
    compact
        .coordinator_mut()
        .enqueue_event(SystemEvent::CompactionStarted {
            source: "slash_compact".to_string(),
        });
    compact.coordinator_mut().drain_transport();
    compact
        .coordinator_mut()
        .enqueue_event(SystemEvent::CompactionCompleted {
            source: "slash_compact".to_string(),
            summary_preview: "preview".to_string(),
            summary_body: "summary body".to_string(),
        });
    compact.coordinator_mut().drain_transport();

    // -- Check: the artifact is visible without a slash echo.
    let lines = compact
        .state()
        .transcript
        .entries()
        .iter()
        .map(entry_text)
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(lines.contains(&"summary body".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains("status=running")));
    assert!(!lines.iter().any(|line| line.starts_with("/compact")));

    // -- Setup & Fixtures: /theme.
    let mut theme = RenderLoopDriver::new(120, 30);
    let script: Vec<DriveEvent> = "/theme"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();

    // -- Exec
    theme.advance(&script).await?;

    // -- Check: the theme picker is open; the launch queue is empty.
    assert_eq!(
        theme.state().picker.render_kind(),
        Some(PickerRenderKind::Theme)
    );
    assert_eq!(
        theme.coordinator_mut().state.take_next_launch_request(),
        None
    );
    assert_eq!(
        theme
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    Ok(())
}

/// Immediate slash commands do not set busy or the spinner.
#[tokio::test]
async fn immediate_slash_commands_do_not_set_busy_or_spinner() -> Result<()> {
    for command in ["/compact", "/mcp", "/help", "/status"] {
        // -- Setup & Fixtures
        let mut driver = RenderLoopDriver::new(120, 30);
        driver.coordinator_mut().textarea =
            ratatui_textarea::TextArea::new(vec![command.to_string()]);

        // -- Exec
        driver.advance(&[key(TerminalKey::Enter)]).await?;

        // -- Check
        assert!(
            driver.orchestrator_events().iter().any(|event| matches!(
                event,
                OrchestratorEvent::PromptSubmitted { text } if text == command
            )),
            "loop must hand the slash command to the orchestrator channel"
        );
        assert_eq!(
            driver
                .coordinator_mut()
                .state
                .take_next_prompt_for_execution(),
            None
        );
        assert_eq!(
            driver.state().phase,
            UiPhase::Idle,
            "immediate command must not transition into Busy"
        );
        assert!(
            !driver.state().is_active_cycle(),
            "immediate command must not activate prompt lifecycle"
        );
        assert!(
            driver.state().status.message.status_line() != "Thinking...",
            "spinner lane status must not be set for immediate slash commands"
        );
    }
    Ok(())
}
