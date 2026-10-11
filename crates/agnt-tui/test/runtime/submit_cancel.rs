use super::*;

#[test]
fn idle_startup_does_not_show_spinner() {
    let coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    assert_ne!(
        coordinator.state().status.message.status_line(),
        "Thinking..."
    );
    assert!(!coordinator.state().input_locked);
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.starts_with("○ "),
        "idle startup must show idle indicator, got {text:?}"
    );
}

#[tokio::test]
async fn coordinator_submit_handoff_keeps_input_editable_and_preserves_transcript_preview()
-> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;

    // -- Check
    assert_eq!(driver.state().phase, UiPhase::Busy);
    assert!(!driver.state().input_locked);
    // The real loop drained the pending prompt into a PromptSubmitted event
    // for the orchestrator; the poll-style queue is empty afterwards.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "x")
        ),
        "loop must hand the prompt to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    // [User] — no leading or trailing spacer under the unified spacer rule
    assert_eq!(driver.state().transcript.len(), 1);
    assert!(matches!(
        driver.state().transcript.blocks()[0].source,
        BlockSource::Markdown {
            role: MessageRole::User,
            ..
        }
    ));
    assert_eq!(
        driver.state().transcript.blocks()[0].source.plain_text(),
        "x"
    );
    Ok(())
}

#[tokio::test]
async fn slash_commands_do_not_append_command_text_to_transcript() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // In the TextArea architecture, char keys are routed to TextArea by
    // handle_insert_mode_key. Set the textarea content directly to simulate
    // the coordinator flow for slash command submission.
    driver.coordinator_mut().textarea = ratatui_textarea::TextArea::new(vec!["/help".to_string()]);

    // -- Exec
    driver.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check
    // The real loop drained the submitted command into a PromptSubmitted
    // event; the poll-style queue is empty afterwards.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "/help")
        ),
        "loop must hand the slash command to the orchestrator channel"
    );
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
    assert!(driver.state().transcript.blocks().is_empty());
    Ok(())
}

#[tokio::test]
async fn compact_result_artifact_is_visible_without_slash_command_echo() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit /compact through the real terminal arm.
    let script: Vec<DriveEvent> = "/compact"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();
    driver.advance(&script).await?;

    // -- Check
    // The real loop drained the submitted command into a PromptSubmitted
    // event; the poll-style queue is empty afterwards.
    assert!(
        driver
            .orchestrator_events()
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "/compact")),
        "loop must hand the slash command to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert!(driver.state().transcript.blocks().is_empty());

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::CompactionStarted {
            source: "slash_compact".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::CompactionCompleted {
            source: "slash_compact".to_string(),
            summary_preview: "preview".to_string(),
            summary_body: "summary body".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    let lines = driver
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(lines.contains(&"summary body".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains("status=running")));
    assert!(!lines.iter().any(|line| line.starts_with("/compact")));
    Ok(())
}

// Regression test for the slash char-drop defect: the keystrokes must be
// routed through the REAL render-loop terminal arm (mpsc channel +
// tokio::select!), not a synthetic poll harness.
#[tokio::test]
async fn slash_prefix_filters_suggestions_and_enter_submits_matching_command() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type /theme and submit through the real terminal arm.
    let script: Vec<DriveEvent> = "/theme"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();
    driver.advance(&script).await?;

    // -- Check
    // The real loop drained the launch request via the pending ui-state
    // events, published it on the bus, and re-reduced it: the theme picker is
    // now open, and the launch queue is empty.
    assert_eq!(
        driver.state().picker.render_kind(),
        Some(PickerRenderKind::Theme),
        "loop must execute the theme launch request through the ui_state arm"
    );
    assert_eq!(
        driver.coordinator_mut().state.take_next_launch_request(),
        None
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    Ok(())
}

#[tokio::test]
async fn immediate_slash_commands_do_not_set_busy_or_spinner() -> Result<()> {
    for command in ["/compact", "/mcp", "/help", "/status"] {
        // -- Setup & Fixtures
        let mut driver = RenderLoopDriver::new(120, 30);

        // In the TextArea architecture, char keys are routed to TextArea by
        // handle_insert_mode_key. Set the textarea content directly to simulate
        // the coordinator flow for slash command submission.
        driver.coordinator_mut().textarea =
            ratatui_textarea::TextArea::new(vec![command.to_string()]);

        // -- Exec
        driver.advance(&[key(TerminalKey::Enter)]).await?;

        // -- Check
        // The real loop drained the submitted command into a PromptSubmitted
        // event; the poll-style queue is empty afterwards.
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

#[tokio::test]
async fn coordinator_esc_then_esc_requests_cancel_signal() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[
            key(TerminalKey::Char('q')),
            key(TerminalKey::Enter),
            key(TerminalKey::Esc),
            key(TerminalKey::Esc),
        ])
        .await?;

    // -- Check
    assert!(driver.state().status.message.status_line().is_empty());
    assert!(driver.coordinator().take_cancel_requested());
    Ok(())
}

#[tokio::test]
async fn enter_on_empty_input_while_busy_cancels_turn_and_submits_queued_prompt() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // Start a turn so the phase is Busy.
    driver
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(driver.state().phase, UiPhase::Busy);

    // Queue a second prompt while the turn is busy.
    let script: Vec<DriveEvent> = "queued"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();
    driver.advance(&script).await?;
    assert_eq!(
        driver.state().pending_prompt_count(),
        1,
        "second prompt must stay queued while the turn is busy"
    );

    // -- Exec: press Enter on empty input.
    driver.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check: the coordinator requested cancel and the queued prompt is
    // still pending (not restored to the input).
    assert!(
        driver.coordinator().take_cancel_requested(),
        "empty Enter while busy with a queued prompt must request cancel"
    );
    assert_eq!(
        driver.state().pending_prompt_count(),
        1,
        "queued prompt must stay queued for the drain path"
    );

    // -- Exec: simulate the cancelled turn completing.
    driver
        .advance(&[DriveEvent::UiEvent(Box::new(UiEvent::Completed {
            tool_calls: 0,
        }))])
        .await?;

    // -- Check: the queued prompt was submitted through the drain path.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "queued")
        ),
        "queued prompt must be submitted after the turn cancels"
    );
    Ok(())
}

#[tokio::test]
async fn enter_on_empty_input_does_not_cancel_without_queued_prompts() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec & Check: idle empty Enter must not request cancel.
    driver.advance(&[key(TerminalKey::Enter)]).await?;
    assert_eq!(driver.state().phase, UiPhase::Idle);
    assert!(
        !driver.coordinator().take_cancel_requested(),
        "idle empty Enter must not request cancel"
    );

    // -- Exec & Check: busy empty Enter with no queued prompts must not
    // request cancel either.
    driver
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(driver.state().phase, UiPhase::Busy);
    assert_eq!(driver.state().pending_prompt_count(), 0);
    driver.advance(&[key(TerminalKey::Enter)]).await?;
    assert!(
        !driver.coordinator().take_cancel_requested(),
        "busy empty Enter without queued prompts must not request cancel"
    );
    assert_eq!(driver.state().phase, UiPhase::Busy);
    Ok(())
}

#[test]
fn runtime_renderer_reuses_eventing_and_preserves_emit_passthrough() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:h,ctrlc,resize:140x35");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStarted);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.flush();

    assert!(runtime_renderer.coordinator().take_cancel_requested());
    assert!(runtime_renderer.coordinator().quit_requested());
    let state = runtime_renderer.coordinator().state();
    assert_eq!(state.phase, UiPhase::Busy);
}

#[tokio::test]
async fn render_loop_driver_take_submitted_prompt_supports_interactive_turn_handoff() -> Result<()>
{
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "hi" through the real terminal arm.
    driver
        .advance(&[
            key(TerminalKey::Char('h')),
            key(TerminalKey::Char('i')),
            key(TerminalKey::Enter),
        ])
        .await?;

    // -- Check
    // The real loop drained the submitted prompt into a PromptSubmitted event
    // for the orchestrator; the poll-style queue is empty afterwards.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "hi")
        ),
        "loop must hand the prompt to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    Ok(())
}

#[tokio::test]
async fn render_loop_driver_quit_requested_reflects_ctrlc_terminal_event() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::CtrlC)]).await?;

    // -- Check: the loop must send the orchestrator Quit event before exiting.
    assert!(
        driver
            .orchestrator_events()
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::Quit)),
        "loop must forward Quit to the orchestrator on Ctrl+C"
    );
    assert!(driver.coordinator().quit_requested());
    Ok(())
}

#[tokio::test]
async fn submit_reaches_orchestrator_channel_through_render_loop_terminal_arm() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "hi" through the real terminal arm; the loop
    // itself takes the pending events and forwards them on the orchestrator
    // channel, exactly as production runs it.
    driver
        .advance(&[
            key(TerminalKey::Char('h')),
            key(TerminalKey::Char('i')),
            key(TerminalKey::Enter),
        ])
        .await?;

    // -- Check
    let events = driver.orchestrator_events();
    let submitted = events
        .iter()
        .filter(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "hi"),
        )
        .count();
    assert_eq!(
        submitted, 1,
        "exactly one PromptSubmitted must reach the orchestrator channel"
    );

    // After the channel event is produced, the poll-style queue is empty
    // because the real loop already drained it.
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    // And the transcript shows the user turn was started.
    assert!(driver.state().transcript.blocks().iter().any(|b| matches!(
        b.source,
        BlockSource::Markdown {
            role: MessageRole::User,
            ..
        }
    ) && b.source.plain_text() == "hi"));
    Ok(())
}

#[tokio::test]
async fn global_abort_cancels_active_and_pending_and_new_submit_starts_fresh() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: submit two prompts, then abort with Esc-Esc through the real
    // terminal arm.
    driver
        .advance(&[
            key(TerminalKey::Char('a')),
            key(TerminalKey::Enter),
            key(TerminalKey::Char('b')),
            key(TerminalKey::Enter),
            key(TerminalKey::Esc),
            key(TerminalKey::Esc),
        ])
        .await?;

    // -- Check
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );

    // The real loop handed each submitted prompt to the orchestrator as a
    // PromptSubmitted event before the abort.
    let submitted = driver.take_orchestrator_events();
    assert!(
        submitted.iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "a")
        ),
        "first prompt must reach the orchestrator before the abort"
    );

    let statuses = driver
        .state()
        .prompt_items()
        .iter()
        .map(|item| item.status)
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![PromptStatus::Cancelled, PromptStatus::Cancelled]
    );

    // After abort, the restored text from the cancelled prompt is applied to
    // the textarea by the render loop's terminal arm on the next terminal
    // event. In the real loop the first prompt was already handed to the
    // orchestrator, so only "b" is restored. The Esc-Esc abort returns the
    // input to Insert mode, so typing resumes without an 'i' chord.
    driver.advance(&[key(TerminalKey::Char('c'))]).await?;
    assert_eq!(
        driver.coordinator_mut().textarea.lines().join("\n"),
        "bc",
        "restored cancelled prompt text must be applied to the textarea by the loop"
    );

    // Submitting through the real terminal arm continues from the restored
    // text; the loop hands the prompt to the orchestrator as a
    // PromptSubmitted event.
    driver.advance(&[key(TerminalKey::Enter)]).await?;
    let submitted = driver.take_orchestrator_events();
    assert!(
        submitted.iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "bc")
        ),
        "restored text must be submitted to the orchestrator after the abort"
    );
    Ok(())
}
