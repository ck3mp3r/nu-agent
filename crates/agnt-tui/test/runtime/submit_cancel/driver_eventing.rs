use super::*;

/// The runtime renderer reuses eventing and preserves emit passthrough.
#[test]
fn runtime_renderer_reuses_eventing_and_preserves_emit_passthrough() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:h,ctrlc,ctrlc,ctrlc,resize:140x35");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStarted);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.flush();

    assert!(runtime_renderer.coordinator().take_cancel_requested());
    assert!(runtime_renderer.coordinator().quit_requested());
    let state = runtime_renderer.coordinator().state();
    assert_eq!(state.phase, UiPhase::Busy);
}

/// The render loop hands a submitted prompt to the orchestrator channel exactly
/// once, forwards Quit on Ctrl+C, and a global abort cancels active and pending
/// prompts and starts fresh.
#[tokio::test]
async fn render_loop_submit_and_abort_handoff() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "hi".
    driver
        .advance(&[
            key(TerminalKey::Char('h')),
            key(TerminalKey::Char('i')),
            key(TerminalKey::Enter),
        ])
        .await?;

    // -- Check: exactly one PromptSubmitted reaches the orchestrator.
    let events = driver.orchestrator_events();
    let submitted = events
        .iter()
        .filter(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "hi"),
        )
        .count();
    assert_eq!(submitted, 1);
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert!(driver.state().transcript.blocks().iter().any(|b| matches!(
        b.source,
        BlockSource::Markdown {
            role: MessageRole::User,
            ..
        }
    ) && b.source.plain_text() == "hi"));

    // -- Setup & Fixtures: Ctrl+C twice (arm, then confirm).
    let mut quitting = RenderLoopDriver::new(120, 30);

    // -- Exec
    quitting
        .advance(&[key(TerminalKey::CtrlC), key(TerminalKey::CtrlC)])
        .await?;

    // -- Check: Quit is forwarded.
    assert!(
        quitting
            .orchestrator_events()
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::Quit))
    );
    assert!(quitting.coordinator().quit_requested());

    // -- Setup & Fixtures: two prompts then Esc-Esc.
    let mut aborting = RenderLoopDriver::new(120, 30);

    // -- Exec
    aborting
        .advance(&[
            key(TerminalKey::Char('a')),
            key(TerminalKey::Enter),
            key(TerminalKey::Char('b')),
            key(TerminalKey::Enter),
            key(TerminalKey::Esc),
            key(TerminalKey::Esc),
        ])
        .await?;

    // -- Check: the queue is drained and both prompts are cancelled.
    assert_eq!(
        aborting
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    let submitted = aborting.take_orchestrator_events();
    assert!(
        submitted.iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "a")
        )
    );
    let statuses = aborting
        .state()
        .prompt_items()
        .iter()
        .map(|item| item.status)
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![PromptStatus::Cancelled, PromptStatus::Cancelled]
    );

    // -- Exec: type 'c' to continue from the restored text.
    aborting.advance(&[key(TerminalKey::Char('c'))]).await?;

    // -- Check: the restored text is applied.
    assert_eq!(aborting.coordinator_mut().textarea.lines().join("\n"), "bc");

    // -- Exec: submit.
    aborting.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check: the restored text is submitted.
    let submitted = aborting.take_orchestrator_events();
    assert!(
        submitted.iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "bc")
        )
    );
    Ok(())
}
