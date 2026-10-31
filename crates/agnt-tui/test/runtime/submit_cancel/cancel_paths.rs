use super::*;

/// The cancel paths: Esc-Esc requests a cancel signal; empty Enter while busy
/// with a queued prompt cancels the turn and submits the queued prompt; empty
/// Enter without queued prompts does not request cancel.
#[tokio::test]
async fn cancel_paths() -> Result<()> {
    // -- Setup & Fixtures: Esc-Esc.
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

    // -- Check: the cancel signal is requested.
    assert!(driver.state().status.message.status_line().is_empty());
    assert!(driver.coordinator().take_cancel_requested());

    // -- Setup & Fixtures: a busy turn with a queued prompt.
    let mut queued = RenderLoopDriver::new(120, 30);
    queued
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(queued.state().phase, UiPhase::Busy);
    let script: Vec<DriveEvent> = "queued"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();
    queued.advance(&script).await?;
    assert_eq!(queued.state().pending_prompt_count(), 1);

    // -- Exec: press Enter on empty input.
    queued.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check: cancel requested; the queued prompt stays queued.
    assert!(queued.coordinator().take_cancel_requested());
    assert_eq!(queued.state().pending_prompt_count(), 1);

    // -- Exec: the cancelled turn completes.
    queued
        .advance(&[DriveEvent::SystemEvent(Box::new(
            SystemEvent::TurnCompleted {
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: None,
                cost: None,
                cost_complete: true,
                duration_ms: 0,
            },
        ))])
        .await?;

    // -- Check: the queued prompt is submitted through the drain path.
    assert!(queued.orchestrator_events().iter().any(
        |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "queued")
    ));

    // -- Setup & Fixtures: idle and busy with no queued prompts.
    let mut none = RenderLoopDriver::new(120, 30);

    // -- Exec & Check: idle empty Enter must not request cancel.
    none.advance(&[key(TerminalKey::Enter)]).await?;
    assert_eq!(none.state().phase, UiPhase::Idle);
    assert!(!none.coordinator().take_cancel_requested());

    // -- Exec & Check: busy empty Enter with no queued prompts must not cancel.
    none.advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(none.state().phase, UiPhase::Busy);
    assert_eq!(none.state().pending_prompt_count(), 0);
    none.advance(&[key(TerminalKey::Enter)]).await?;
    assert!(!none.coordinator().take_cancel_requested());
    assert_eq!(none.state().phase, UiPhase::Busy);
    Ok(())
}
