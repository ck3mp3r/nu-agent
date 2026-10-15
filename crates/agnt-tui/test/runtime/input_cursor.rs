use super::*;

#[test]
fn cursor_style_maps_insert_to_bar_and_normal_visual_to_block() {
    assert!(matches!(
        cursor_style_for_test(InputMode::Insert),
        crossterm::cursor::SetCursorStyle::SteadyBar
    ));
    assert!(matches!(
        cursor_style_for_test(InputMode::Normal),
        crossterm::cursor::SetCursorStyle::SteadyBlock
    ));
    assert!(matches!(
        cursor_style_for_test(InputMode::Visual),
        crossterm::cursor::SetCursorStyle::SteadyBlock
    ));
}

/// The prompt prefix uses the mode indicator (insert vs normal/visual), appears
/// only on the first visual row, switches immediately on a mode change, and the
/// content width subtracts only the 2-char prompt prefix.
#[test]
fn prompt_prefix_and_content_width() -> Result<()> {
    // -- Setup & Fixtures
    let insert = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };
    let normal = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    let visual = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        ..Default::default()
    };

    // -- Exec & Check: the mode indicator.
    assert_eq!(input_rows_with_prompt_for_test(&insert, 20), vec!["❯ "]);
    assert_eq!(input_rows_with_prompt_for_test(&normal, 20), vec!["❮ "]);
    assert_eq!(input_rows_with_prompt_for_test(&visual, 20), vec!["❮ "]);

    // -- Exec & Check: the icon appears only on the first visual row.
    let rows = input_rows_with_prompt_for_test(&insert, 5);
    assert_eq!(rows, vec!["❯ "]);

    // -- Exec & Check: the prefix switches immediately on a mode change.
    let mut switching = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };
    assert_eq!(input_rows_with_prompt_for_test(&switching, 20), vec!["❯ "]);
    switching.input.mode = InputMode::Normal;
    assert_eq!(input_rows_with_prompt_for_test(&switching, 20), vec!["❮ "]);

    // -- Exec & Check: the content width subtracts only the prompt prefix.
    let inner_width: u16 = 10;
    assert_eq!(input_pane_content_width_for_test(inner_width), 8);
    let rows = wrapped_input_rows("abcdefghi", input_pane_content_width_for_test(inner_width));
    assert_eq!(rows, vec!["abcdefgh", "i"]);
    Ok(())
}

/// `new_textarea` and the coordinator default enable WordOrGlyph soft wrap.
#[test]
fn textarea_wrap_mode_word_or_glyph() -> Result<()> {
    // -- Setup & Fixtures
    let lines = vec!["hello world".to_string(), "second line".to_string()];

    // -- Exec & Check: a populated textarea.
    let textarea = RuntimeCoordinator::new_textarea(lines);
    assert_eq!(
        textarea.wrap_mode(),
        ratatui_textarea::WrapMode::WordOrGlyph
    );

    // -- Exec & Check: an empty textarea.
    let empty = RuntimeCoordinator::new_textarea(Vec::new());
    assert_eq!(empty.wrap_mode(), ratatui_textarea::WrapMode::WordOrGlyph);

    // -- Exec & Check: the coordinator default.
    let coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    assert_eq!(
        coordinator.textarea.wrap_mode(),
        ratatui_textarea::WrapMode::WordOrGlyph
    );
    Ok(())
}

/// The input box height grows with the wrapped rows while typing, uses the
/// crate's real wrap width, and recomputes on resize.
#[tokio::test]
async fn input_height_wraps_and_recomputes() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: type a sentence that wraps at the narrow content width.
    let script: Vec<DriveEvent> = "hello world foo bar"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance(&script).await?;

    // -- Check: the box grows to the wrapped visual row count.
    assert_eq!(driver.coordinator().input_height, 2);

    // -- Setup & Fixtures: a 15-char line at the crate's real width.
    let mut boundary = RenderLoopDriver::new(20, 30);
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();

    // -- Exec
    boundary.advance(&script).await?;

    // -- Check: the crate wraps at 14 → 2 rows.
    assert_eq!(boundary.coordinator().input_height, 2);

    // -- Setup & Fixtures: a wide terminal.
    let mut resizing = RenderLoopDriver::new(120, 30);
    let script: Vec<DriveEvent> = "hello world foo bar baz qux"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    resizing.advance(&script).await?;
    assert_eq!(resizing.coordinator().input_height, 1);

    // -- Exec: resize to a narrow width.
    resizing
        .advance(&[DriveEvent::Key(TerminalEvent::Resize(
            crate::interaction::input::TerminalResize {
                columns: 20,
                rows: 30,
            },
        ))])
        .await?;

    // -- Check: the box height grows to the wrapped row count.
    assert!(resizing.coordinator().input_height > 1);
    Ok(())
}

/// The terminal cursor renders on the correct visual row for a wrapped line, an
/// alt-return new logical line, and an internally scrolled input.
#[tokio::test]
async fn screen_cursor_positions_on_visual_row() -> Result<()> {
    // -- Setup & Fixtures: a wrapped line.
    let mut wrapped = RenderLoopDriver::new(20, 30);
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();

    // -- Exec
    wrapped.advance_with_frame(&script).await?;

    // -- Check: the caret sits on the second visual row.
    let buffer = wrapped.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    assert_eq!(wrapped.cursor_position().y, indicator_y + 1);

    // -- Setup & Fixtures: a wrapping line plus AltEnter.
    let mut alt = RenderLoopDriver::new(20, 30);
    let mut script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    script.push(key(TerminalKey::AltEnter));

    // -- Exec
    alt.advance_with_frame(&script).await?;

    // -- Check: the new logical line starts at visual row 2.
    let buffer = alt.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    assert_eq!(alt.cursor_position().y, indicator_y + 2);

    // -- Setup & Fixtures: a long single word that wraps to >6 visual rows.
    let mut scrolled = RenderLoopDriver::new(20, 30);
    let script: Vec<DriveEvent> = "x"
        .repeat(100)
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();

    // -- Exec
    scrolled.advance_with_frame(&script).await?;

    // -- Check: the caret is on the last visible row.
    let buffer = scrolled.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    assert_eq!(scrolled.cursor_position().y, indicator_y + 5);
    Ok(())
}

/// History navigation: Up mid-wrap moves the cursor within the textarea, Up on
/// the first visual row loads history, and Down on the last visual row restores
/// the saved draft.
#[tokio::test]
async fn history_navigation_visual_row_boundaries() -> Result<()> {
    // -- Setup & Fixtures: a wrapping line, caret on the second visual row.
    let mut mid = RenderLoopDriver::new(20, 30);
    mid.coordinator_mut()
        .state
        .enqueue_prompt("prev".to_string());
    let _ = mid.coordinator_mut().state.activate_next_prompt();
    mid.coordinator_mut().state.complete_active_prompt();
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    mid.advance(&script).await?;
    assert_eq!(mid.coordinator_mut().textarea.screen_cursor().row, 1);

    // -- Exec: press Up mid-way through the wrapped first line.
    mid.advance(&[key(TerminalKey::Up)]).await?;

    // -- Check: history is not loaded; the cursor moves within the textarea.
    assert_eq!(
        mid.coordinator_mut().textarea.lines().join("\n"),
        "hello world foo"
    );
    assert_eq!(mid.coordinator_mut().textarea.screen_cursor().row, 0);

    // -- Setup & Fixtures: a wrapping line, caret on the second visual row.
    let mut down = RenderLoopDriver::new(20, 30);
    down.coordinator_mut()
        .state
        .enqueue_prompt("prev".to_string());
    let _ = down.coordinator_mut().state.activate_next_prompt();
    down.coordinator_mut().state.complete_active_prompt();
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    down.advance(&script).await?;
    assert_eq!(down.coordinator_mut().textarea.screen_cursor().row, 1);

    // -- Exec: Up twice loads history, then Down restores the draft.
    down.advance(&[key(TerminalKey::Up), key(TerminalKey::Up)])
        .await?;
    assert_eq!(down.coordinator_mut().textarea.lines().join("\n"), "prev");
    down.advance(&[key(TerminalKey::Down)]).await?;
    assert_eq!(
        down.coordinator_mut().textarea.lines().join("\n"),
        "hello world foo"
    );

    // -- Setup & Fixtures: a short line on one row.
    let mut short = RenderLoopDriver::new(20, 30);
    short
        .coordinator_mut()
        .state
        .enqueue_prompt("prev".to_string());
    let _ = short.coordinator_mut().state.activate_next_prompt();
    short.coordinator_mut().state.complete_active_prompt();
    let script: Vec<DriveEvent> = "hi".chars().map(|c| key(TerminalKey::Char(c))).collect();
    short.advance(&script).await?;
    assert_eq!(short.coordinator_mut().textarea.screen_cursor().row, 0);

    // -- Exec
    short.advance(&[key(TerminalKey::Up)]).await?;

    // -- Check: history is loaded.
    assert_eq!(short.coordinator_mut().textarea.lines().join("\n"), "prev");
    Ok(())
}

#[tokio::test]
async fn render_loop_driver_wrapped_prompt_round_trips_exactly_on_submit() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);
    let typed = "hello world foo bar baz qux";

    // -- Exec: type a line that wraps at the narrow content width, then submit.
    let mut script: Vec<DriveEvent> = typed.chars().map(|c| key(TerminalKey::Char(c))).collect();
    script.push(key(TerminalKey::Enter));
    driver.advance(&script).await?;

    // -- Check: the submitted text matches the typed text exactly; wrap is
    // visual only and must not insert newlines.
    let submitted = driver
        .orchestrator_events()
        .iter()
        .filter_map(|event| match event {
            OrchestratorEvent::PromptSubmitted { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        submitted,
        vec![typed],
        "wrapped prompt must round-trip exactly on submit"
    );
    Ok(())
}

#[tokio::test]
async fn render_loop_driver_replace_textarea_resets_scroll_top_mirror() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: scroll first by typing a long single word that wraps to >6
    // visual rows, so the scroll-top mirror becomes nonzero.
    let script: Vec<DriveEvent> = "x"
        .repeat(100)
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance_with_frame(&script).await?;
    assert!(
        driver.coordinator().last_input_scroll_top > 0,
        "scrolled frame must leave a nonzero scroll-top mirror"
    );

    // Replace the textarea via cancel-restore with a shorter buffer whose
    // caret lands at screen row 2 (0 < 2 < height). The next terminal event
    // triggers pickup_restored_input_text, which replaces the textarea.
    driver.coordinator_mut().state.input.restored_input_text =
        Some("hello world foo bar baz qux".to_string());
    driver
        .advance_with_frame(&[key(TerminalKey::Char('z'))])
        .await?;

    // -- Check: the caret's rendered row must be indicator_y + 2, not
    // indicator_y + (2 - stale_top).
    let buffer = driver.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    let pos = driver.cursor_position();
    assert_eq!(
        pos.y,
        indicator_y + 2,
        "replaced textarea must render the caret on its own screen row, not a stale scroll offset"
    );
    Ok(())
}
