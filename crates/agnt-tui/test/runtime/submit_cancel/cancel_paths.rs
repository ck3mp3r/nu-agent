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
        .advance(&[DriveEvent::Record(Box::new(turn_completed(0)))])
        .await?;

    // -- Check: the queued prompt is submitted through the drain path.
    assert!(queued.orchestrator_events().iter().any(
        |event| matches!(event, OrchestratorEvent::PromptSubmitted { text, .. } if text == "queued")
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

/// A surface above a running turn ends its own Esc: the inline slash list
/// closes and nothing else changes. The next bare Esc arms, and the one after
/// that confirms.
#[tokio::test]
async fn busy_escape_closes_inline_slash_only() -> Result<()> {
    // -- Setup & Fixtures: a running turn with the inline slash list open.
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(driver.state().phase, UiPhase::Busy);
    driver.advance(&[key(TerminalKey::Char('/'))]).await?;
    assert_eq!(
        driver.state().picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );

    // -- Exec: one Esc.
    driver.advance(&[key(TerminalKey::Esc)]).await?;

    // -- Check: the list closed and the press ended at the surface.
    assert_eq!(driver.state().picker.render_kind(), None);
    assert_eq!(driver.state().phase, UiPhase::Busy);
    assert!(!driver.state().abort.pending);
    assert!(driver.state().status.message.status_line().is_empty());
    assert!(!driver.coordinator().take_cancel_requested());

    // -- Exec: the second Esc is bare and arms the abort.
    driver.advance(&[key(TerminalKey::Esc)]).await?;

    // -- Check
    assert_eq!(driver.state().phase, UiPhase::AbortPending);
    assert_eq!(
        driver.state().status.message.status_line(),
        ESC_ABORT_CONFIRM_STATUS
    );
    assert!(!driver.coordinator().take_cancel_requested());

    // -- Exec: the third Esc confirms the armed abort.
    driver.advance(&[key(TerminalKey::Esc)]).await?;

    // -- Check
    assert!(driver.coordinator().take_cancel_requested());
    assert!(driver.state().status.message.status_line().is_empty());
    Ok(())
}
