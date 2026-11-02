use super::*;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::{TerminalMouse, TerminalResize};
use crate::runtime::TerminalEventSource;
use ratatui::layout::Rect;

/// An idle 'q' is regular input and never requests quit, through either the
/// direct or the dispatch path.
#[tokio::test]
async fn idle_q_is_regular_input_and_never_requests_quit() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec & Check: a single idle 'q' never requests quit.
    driver.advance(&[key(TerminalKey::Char('q'))]).await?;
    assert!(!driver.coordinator().quit_requested());
    assert!(!driver.state().input_locked);

    // -- Exec & Check: typing 'a' then 'q' still never requests quit.
    driver
        .advance(&[key(TerminalKey::Char('a')), key(TerminalKey::Char('q'))])
        .await?;
    assert!(!driver.coordinator().quit_requested());

    // -- Setup & Fixtures: the dispatch path.
    let mut dispatch = RenderLoopDriver::new(120, 30);

    // -- Exec
    dispatch.advance(&[key(TerminalKey::Char('q'))]).await?;

    // -- Check
    assert!(!dispatch.coordinator().quit_requested());
    Ok(())
}

/// The crossterm event source returns none when idle, and the Enter modifier
/// mapping distinguishes submit vs newline intents.
#[test]
fn crossterm_event_source_and_enter_modifier_mapping() -> Result<()> {
    // -- Exec & Check: a zero-timeout source returns none when idle.
    if unsafe { libc::isatty(libc::STDIN_FILENO) } != 0 {
        let mut source =
            crate::runtime::CrosstermTerminalEvents::new(std::time::Duration::from_millis(0));
        assert_eq!(source.poll_event(), Ok(None));
    }

    // -- Exec & Check: the Enter modifier mapping.
    let plain = crate::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert_eq!(plain, Some(TerminalEvent::Key(TerminalKey::Enter)));

    let alt = crate::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::ALT,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }));
    assert_eq!(alt, Some(TerminalEvent::Key(TerminalKey::AltEnter)));

    let shift = crate::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }));
    assert_eq!(shift, Some(TerminalEvent::Key(TerminalKey::ShiftEnter)));
    Ok(())
}

// region:    --- Busy typing

/// WHEN a turn runs and a character key arrives in insert mode, THE coordinator
/// SHALL write it into the draft; Backspace still edits the same draft.
#[test]
fn coordinator_char_while_input_locked_lands_in_draft() -> Result<()> {
    // -- Setup & Fixtures: a running turn, locked input, empty draft.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.phase = UiPhase::Busy;
    coordinator.state.input_locked = true;
    assert!(!coordinator.state.permission.has_prompt());
    assert_eq!(coordinator.textarea.lines().join("\n"), "");

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Char('x')));

    // -- Check: the typed char reached the draft and the frame is dirty.
    assert_eq!(coordinator.textarea.lines().join("\n"), "x");
    assert!(coordinator.render_needed());

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Backspace));

    // -- Check: Backspace keeps editing the locked draft.
    assert_eq!(coordinator.textarea.lines().join("\n"), "");
    Ok(())
}

/// WHEN Enter arrives while a turn runs and the draft holds text, THE
/// coordinator SHALL enqueue it as one pending prompt, clear the draft, and
/// leave no dispatched-submit residue behind after the settle drain.
#[test]
fn coordinator_enter_while_input_locked_queues_prompt_and_clears_draft() -> Result<()> {
    // -- Setup & Fixtures: a running turn with a typed draft.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.phase = UiPhase::Busy;
    coordinator.state.input_locked = true;
    coordinator.textarea.insert_str("typed while busy");
    assert_eq!(coordinator.state.pending_prompt_count(), 0);

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Enter));

    // -- Check: one queued prompt, empty draft, no residue.
    assert_eq!(coordinator.textarea.lines().join("\n"), "");
    assert_eq!(coordinator.state.pending_prompt_count(), 1);
    assert_eq!(coordinator.state.input.pending_submit_text, None);

    // -- Exec: the turn settles and the pending events drain.
    coordinator.reduce_system_event(SystemEvent::TurnCompleted {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });
    let events = coordinator
        .state
        .take_pending_events(&CancelController::default());

    // -- Check: the queued prompt is delivered exactly once.
    let submitted: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            OrchestratorEvent::PromptSubmitted { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(submitted, vec!["typed while busy"]);
    assert_eq!(coordinator.state.pending_prompt_count(), 0);
    assert_eq!(coordinator.state.input.pending_submit_text, None);
    Ok(())
}

/// WHEN AltEnter or ShiftEnter arrives while a turn runs, THE coordinator SHALL
/// add one newline to the draft and queue nothing: only Enter submits.
#[test]
fn coordinator_newline_keys_while_input_locked_edit_draft_without_queueing() -> Result<()> {
    // -- Setup & Fixtures: a running turn with a typed draft.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.phase = UiPhase::Busy;
    coordinator.state.input_locked = true;
    coordinator.textarea.insert_str("line");

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::AltEnter));
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::ShiftEnter));

    // -- Check: two newlines, nothing queued.
    assert_eq!(coordinator.textarea.lines().join("\n"), "line\n\n");
    assert_eq!(coordinator.state.pending_prompt_count(), 0);
    assert_eq!(coordinator.state.input.pending_submit_text, None);
    Ok(())
}

// endregion: --- Busy typing

// region:    --- Render gating

/// A wheel-up that cannot move the scroll state leaves the frame clean.
#[test]
fn coordinator_handle_terminal_event_noop_wheel_normal_mode_leaves_render_clean() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = wheel_noop_coordinator(InputMode::Normal);

    // -- Exec
    coordinator.handle_terminal_event(wheel_up());

    // -- Check
    assert!(!coordinator.render_needed());
    assert_eq!(coordinator.state.scroll.cursor_visual_row, 0);
    assert_eq!(coordinator.state.scroll.scroll_offset, 0);
    assert!(!coordinator.state.scroll.following_tail);
    Ok(())
}

/// A wheel that moves the scroll state marks the frame dirty.
#[test]
fn coordinator_handle_terminal_event_effective_wheel_marks_render() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = wheel_noop_coordinator(InputMode::Normal);
    push_entry(&mut coordinator, None);

    // -- Exec
    coordinator.handle_terminal_event(wheel_down());

    // -- Check: the wheel resumed tail following, so the frame is dirty.
    assert!(coordinator.state.scroll.following_tail);
    assert!(coordinator.render_needed());
    Ok(())
}

/// A resize mutates the layout, so it always marks the frame dirty.
#[test]
fn coordinator_handle_terminal_event_resize_marks_render() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_render_needed(false);

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Resize(TerminalResize {
        columns: 100,
        rows: 40,
    }));

    // -- Check
    assert!(coordinator.render_needed());
    Ok(())
}

/// A paste in insert mode mutates the textarea, so it marks the frame dirty.
#[test]
fn coordinator_handle_terminal_event_paste_insert_mode_marks_render() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    assert_eq!(coordinator.state.input.mode, InputMode::Insert);
    coordinator.set_render_needed(false);

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Paste("hi".to_string()));

    // -- Check
    assert_eq!(coordinator.textarea.lines().join("\n"), "hi");
    assert!(coordinator.render_needed());
    Ok(())
}

/// An Esc with nothing to close or abort leaves the frame clean.
#[test]
fn coordinator_handle_terminal_event_esc_normal_mode_leaves_render_clean() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = wheel_noop_coordinator(InputMode::Normal);
    assert_eq!(coordinator.state.phase, UiPhase::Idle);
    assert!(coordinator.state.picker.active().is_none());
    assert!(coordinator.state.info_panel.is_none());
    assert!(!coordinator.state.permission.has_prompt());

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Esc));

    // -- Check
    assert!(!coordinator.render_needed());
    Ok(())
}

/// A no-op wheel in insert mode still marks the frame dirty: the insert→normal
/// transition changes the mode indicator and pane focus.
#[test]
fn coordinator_handle_terminal_event_noop_wheel_insert_mode_marks_render() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = wheel_noop_coordinator(InputMode::Insert);

    // -- Exec
    coordinator.handle_terminal_event(wheel_up());

    // -- Check: the scroll did not move, the mode did.
    assert_eq!(coordinator.state.input.mode, InputMode::Normal);
    assert_eq!(coordinator.state.scroll.cursor_visual_row, 0);
    assert!(coordinator.render_needed());
    Ok(())
}

// endregion: --- Render gating

// region:    --- Test Support

/// Coordinator with a rendered transcript area and a scroll position a
/// wheel-up cannot move.
fn wheel_noop_coordinator(mode: InputMode) -> RuntimeCoordinator {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.input.mode = mode;
    coordinator.state.scroll.transcript_area = Rect::new(0, 0, 80, 20);
    coordinator.state.scroll.cursor_visual_row = 0;
    coordinator.state.scroll.scroll_offset = 0;
    coordinator.state.scroll.max_scroll = 0;
    coordinator.state.scroll.following_tail = false;
    coordinator.set_render_needed(false);
    coordinator
}

fn wheel_up() -> TerminalEvent {
    TerminalEvent::Mouse(TerminalMouse {
        up: true,
        column: 5,
        row: 5,
    })
}

fn wheel_down() -> TerminalEvent {
    TerminalEvent::Mouse(TerminalMouse {
        up: false,
        column: 5,
        row: 5,
    })
}

// endregion: --- Test Support
