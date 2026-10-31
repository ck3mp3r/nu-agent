use super::*;

use crate::interaction::input::{TerminalMouse, TerminalResize};
use ratatui::layout::Rect;

// Sync-path coverage: the input-error handling lives in `poll_terminal_event`,
// which the async render loop's healthy mpsc channel cannot produce. These
// tests call the production poll + drain primitives directly (the same calls
// `TuiRuntimeRenderer` makes).

/// `poll_terminal_event` picks up a restored input text (replacing the textarea
/// and moving the cursor to the end) and leaves the textarea unchanged when
/// there is none.
#[test]
fn coordinator_poll_terminal_event_pickup_restored_input_text() -> Result<()> {
    // -- Setup & Fixtures: a restored text.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.input.restored_input_text = Some("restored".to_string());
    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "crossterm",
            primary_available: Some(true),
            fallback_available: Some(false),
            last_poll_state: "idle".to_string(),
            last_error: None,
        },
    };

    // -- Exec
    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    // -- Check: the restored text replaces the textarea content.
    assert_eq!(coordinator.textarea.lines().join("\n"), "restored");
    assert_eq!(
        coordinator.textarea.cursor(),
        ratatui_textarea::DataCursor(0, 8)
    );
    assert!(coordinator.render_needed);

    // -- Setup & Fixtures: no restored text.
    let mut none = RuntimeCoordinator::new(120, 30, Some(true));
    none.textarea.insert_str("seeded");
    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "crossterm",
            primary_available: Some(true),
            fallback_available: Some(false),
            last_poll_state: "idle".to_string(),
            last_error: None,
        },
    };

    // -- Exec
    none.poll_terminal_event(&mut source);
    none.drain_transport();

    // -- Check: the textarea is unchanged.
    assert_eq!(none.textarea.lines().join("\n"), "seeded");
    Ok(())
}

/// A terminal input error surfaces a fatal status and requests quit; the
/// watchdog and immediate-poll paths fail fast with an actionable message when
/// no backend is available; the diagnostics snapshot reports the backend state.
#[test]
fn coordinator_input_error_and_watchdog_fail_fast() -> Result<()> {
    // -- Setup & Fixtures: a plain source error.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = ErrorEventSource;

    // -- Exec
    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    // -- Check
    assert!(coordinator.quit_requested());
    assert!(coordinator.take_cancel_requested());
    assert_eq!(
        coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
    assert!(
        coordinator.state.status.message.status_line().is_empty(),
        "the poll-error message never renders in the status lane"
    );

    // -- Setup & Fixtures: the runtime renderer with a failing source.
    let inner = FakeRenderer::default();
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, ErrorEventSource, 120, 30);

    // -- Exec
    runtime_renderer.emit(&SystemEvent::Tick);

    // -- Check: the failure is surfaced.
    assert!(runtime_renderer.quit_requested());
    assert_eq!(
        runtime_renderer.coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );

    // -- Setup & Fixtures: the watchdog with no backend.
    let mut watchdog = RuntimeCoordinator::new_for_test_with_watchdog(
        120,
        30,
        Some(true),
        std::time::Duration::from_millis(0),
    );
    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "none",
            primary_available: Some(false),
            fallback_available: Some(false),
            last_poll_state: "crossterm error; /dev/tty unavailable".to_string(),
            last_error: Some("crossterm poll failed".to_string()),
        },
    };

    // -- Exec
    watchdog.poll_terminal_event(&mut source);
    watchdog.drain_transport();

    // -- Check: the watchdog fails fast with an actionable message.
    let fatal = watchdog
        .fatal_error()
        .ok_or("should have watchdog fatal error")?;
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed"));
    assert!(fatal.contains("interactive terminal"));
    assert!(watchdog.quit_requested());
    assert!(
        watchdog.state.status.message.status_line().is_empty(),
        "the fail-fast message never renders in the status lane"
    );

    // -- Setup & Fixtures: a diagnostics-only source.
    let mut diagnostics = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "tty",
            primary_available: Some(false),
            fallback_available: Some(true),
            last_poll_state: "crossterm error; /dev/tty delivered event".to_string(),
            last_error: Some("crossterm poll failed: EIO".to_string()),
        },
    };

    // -- Exec
    diagnostics.poll_terminal_event(&mut source);
    diagnostics.drain_transport();

    // -- Check: the diagnostics snapshot reports the backend state.
    let (backend, last_poll, last_error) = diagnostics.input_diagnostics_snapshot();
    assert_eq!(
        backend,
        "active=tty, crossterm=unavailable, /dev/tty=available"
    );
    assert_eq!(last_poll, "crossterm error; /dev/tty delivered event");
    assert_eq!(last_error.as_deref(), Some("crossterm poll failed: EIO"));

    // -- Setup & Fixtures: an immediate poll error with no backends.
    let mut immediate = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = ErrorWithDiagnosticsEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "none",
            primary_available: Some(false),
            fallback_available: Some(false),
            last_poll_state: "crossterm error; /dev/tty unavailable".to_string(),
            last_error: None,
        },
        error: "crossterm poll failed: not a terminal".to_string(),
    };

    // -- Exec
    immediate.poll_terminal_event(&mut source);
    immediate.drain_transport();

    // -- Check: the immediate poll error fails fast with an actionable message.
    let fatal = immediate
        .fatal_error()
        .ok_or("should have fatal fail-fast error")?;
    assert!(immediate.quit_requested());
    assert!(immediate.take_cancel_requested());
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed: not a terminal"));
    assert!(fatal.contains("Run `agent` in an interactive terminal"));
    assert!(!fatal.contains("Terminal input error:"));
    assert!(
        immediate.state.status.message.status_line().is_empty(),
        "the fail-fast message never renders in the status lane"
    );
    Ok(())
}

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

/// An empty transport drain leaves the render flag untouched.
#[test]
fn coordinator_drain_transport_empty_leaves_render_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_render_needed(false);

    // -- Exec
    coordinator.drain_transport();

    // -- Check: a clean flag stays clean.
    assert!(!coordinator.render_needed());

    // -- Setup & Fixtures: a dirty flag.
    coordinator.set_render_needed(true);

    // -- Exec
    coordinator.drain_transport();

    // -- Check: a dirty flag stays dirty.
    assert!(coordinator.render_needed());
    Ok(())
}

/// A transport item whose reduce reports changed marks the frame dirty; one
/// that reports no change does not.
#[test]
fn coordinator_drain_transport_changed_item_marks_render() -> Result<()> {
    // -- Setup & Fixtures: a SystemEvent that reduces to true.
    let mut changed = RuntimeCoordinator::new(120, 30, Some(true));
    changed.enqueue_event(SystemEvent::Tick);
    changed.set_render_needed(false);

    // -- Exec
    changed.drain_transport();

    // -- Check
    assert!(changed.render_needed());

    // -- Setup & Fixtures: a SystemEvent that reduces to false.
    let mut unchanged = RuntimeCoordinator::new(120, 30, Some(true));
    unchanged.enqueue_event(SystemEvent::PermissionDecisionSubmitted {
        request_id: RequestId::new("ask-0000000000000001"),
        decision: PermissionDecision::AllowOnce,
        matched_rule_identity: "nested:nu.command:*".to_string(),
    });
    unchanged.set_render_needed(false);

    // -- Exec
    unchanged.drain_transport();

    // -- Check
    assert!(!unchanged.render_needed());
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
