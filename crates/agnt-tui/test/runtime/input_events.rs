use super::*;

// Sync-path coverage: the input-error handling lives in `poll_terminal_event`,
// which the async render loop's healthy mpsc channel cannot produce. These
// tests call the production poll + drain primitives directly (the same calls
// `TuiRuntimeRenderer` makes).
#[test]
fn coordinator_poll_terminal_event_pickup_restored_input_text_some() -> Result<()> {
    // -- Setup & Fixtures
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

    // -- Check
    assert_eq!(
        coordinator.textarea.lines().join("\n"),
        "restored",
        "restored_input_text must replace the textarea content"
    );
    assert_eq!(
        coordinator.textarea.cursor(),
        ratatui_textarea::DataCursor(0, 8),
        "cursor must jump to the end of the restored single-line value"
    );
    assert!(
        coordinator.render_needed,
        "pickup must mark a render as needed"
    );
    Ok(())
}

#[test]
fn coordinator_poll_terminal_event_pickup_restored_input_text_none() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.textarea.insert_str("seeded");
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

    // -- Check
    assert_eq!(
        coordinator.textarea.lines().join("\n"),
        "seeded",
        "textarea must be unchanged when restored_input_text is None"
    );
    Ok(())
}

#[test]
fn coordinator_terminal_input_error_surfaces_status_and_requests_quit() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = ErrorEventSource;

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    assert!(coordinator.quit_requested());
    assert!(coordinator.take_cancel_requested());
    assert_eq!(
        coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
}

#[test]
fn runtime_renderer_reports_fatal_error_on_event_source_failure() {
    let inner = FakeRenderer::default();
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, ErrorEventSource, 120, 30);

    // emit() runs the production poll + drain + render cycle, which surfaces
    // the event source failure.
    runtime_renderer.emit(&UiEvent::Tick);

    assert!(runtime_renderer.quit_requested());
    assert_eq!(
        runtime_renderer.coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
}

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
    Ok(())
}

#[tokio::test]
async fn idle_q_does_not_quit_through_dispatch_path() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::Char('q'))]).await?;

    // -- Check
    assert!(!driver.coordinator().quit_requested());
    Ok(())
}

#[test]
fn watchdog_fails_fast_when_no_input_backend_available() -> Result<()> {
    let mut coordinator = RuntimeCoordinator::new_for_test_with_watchdog(
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

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    let fatal = coordinator
        .fatal_error()
        .ok_or("should have watchdog fatal error")?;
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed"));
    assert!(fatal.contains("interactive terminal"));
    assert!(coordinator.quit_requested());
    Ok(())
}

#[test]
fn diagnostics_snapshot_reports_active_backend_last_poll_and_last_error() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "tty",
            primary_available: Some(false),
            fallback_available: Some(true),
            last_poll_state: "crossterm error; /dev/tty delivered event".to_string(),
            last_error: Some("crossterm poll failed: EIO".to_string()),
        },
    };

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    let (backend, last_poll, last_error) = coordinator.input_diagnostics_snapshot();
    assert_eq!(
        backend,
        "active=tty, crossterm=unavailable, /dev/tty=available"
    );
    assert_eq!(last_poll, "crossterm error; /dev/tty delivered event");
    assert_eq!(last_error.as_deref(), Some("crossterm poll failed: EIO"));
}

#[test]
fn immediate_poll_error_fails_fast_with_actionable_message_when_no_backends_available() -> Result<()>
{
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

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

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    let fatal = coordinator
        .fatal_error()
        .ok_or("should have fatal fail-fast error")?;
    assert!(coordinator.quit_requested());
    assert!(coordinator.take_cancel_requested());
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed: not a terminal"));
    assert!(fatal.contains("Run `agent` in an interactive terminal"));
    assert!(!fatal.contains("Terminal input error:"));
    Ok(())
}

#[test]
fn crossterm_event_source_with_zero_timeout_returns_none_when_idle() {
    if unsafe { libc::isatty(libc::STDIN_FILENO) } == 0 {
        return; // no TTY available (e.g. Nix sandbox)
    }
    let mut source =
        crate::runtime::CrosstermTerminalEvents::new(std::time::Duration::from_millis(0));

    let event = source.poll_event();
    assert_eq!(event, Ok(None));
}

#[test]
fn crossterm_enter_modifier_mapping_distinguishes_submit_vs_newline_intents() {
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
}
