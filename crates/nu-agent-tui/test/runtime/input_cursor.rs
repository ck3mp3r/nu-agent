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

#[test]
fn multiline_input_prompt_icon_appears_only_on_first_visual_row() {
    let state = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };

    let rows = input_rows_with_prompt_for_test(&state, 5);
    assert_eq!(rows, vec!["❯ "]);
}

#[test]
fn prompt_prefix_uses_mode_indicator_insert_vs_normal_visual() {
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

    assert_eq!(input_rows_with_prompt_for_test(&insert, 20), vec!["❯ "]);
    assert_eq!(input_rows_with_prompt_for_test(&normal, 20), vec!["❮ "]);
    assert_eq!(input_rows_with_prompt_for_test(&visual, 20), vec!["❮ "]);
}

#[test]
fn prompt_prefix_switches_immediately_when_mode_changes() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };

    assert_eq!(input_rows_with_prompt_for_test(&state, 20), vec!["❯ "]);

    state.input.mode = InputMode::Normal;
    assert_eq!(input_rows_with_prompt_for_test(&state, 20), vec!["❮ "]);
}

#[test]
fn input_content_width_accounts_for_borders() {
    // The input area sits inside the outer unified rounded box. The outer box
    // already provides the enclosing border, so only the 2-char prompt prefix
    // ("❯ ") is subtracted from the inner width. The call site must therefore
    // pass `inner_width - 2` (not `pane_width - 4`) to wrapped_input_rows.
    let inner_width: u16 = 10;
    assert_eq!(
        input_pane_content_width_for_test(inner_width),
        8,
        "content_width must be inner_width - 2 (prompt-prefix only, no inner border)"
    );

    // Also verify that wrapping at the correct width splits a 9-char string.
    let rows = wrapped_input_rows("abcdefghi", input_pane_content_width_for_test(inner_width));
    assert_eq!(
        rows,
        vec!["abcdefgh", "i"],
        "9-char input must wrap into 2 rows when content_width is 8"
    );
}

#[test]
fn new_textarea_wrap_mode_word_or_glyph() -> Result<()> {
    // -- Setup & Fixtures
    let lines = vec!["hello world".to_string(), "second line".to_string()];

    // -- Exec
    let textarea = RuntimeCoordinator::new_textarea(lines);

    // -- Check
    assert_eq!(
        textarea.wrap_mode(),
        ratatui_textarea::WrapMode::WordOrGlyph,
        "new_textarea must enable WordOrGlyph soft wrap"
    );
    Ok(())
}

#[test]
fn new_textarea_wrap_mode_word_or_glyph_empty_lines() -> Result<()> {
    // -- Setup & Fixtures
    let lines: Vec<String> = Vec::new();

    // -- Exec
    let textarea = RuntimeCoordinator::new_textarea(lines);

    // -- Check
    assert_eq!(
        textarea.wrap_mode(),
        ratatui_textarea::WrapMode::WordOrGlyph,
        "new_textarea must enable WordOrGlyph soft wrap for empty input"
    );
    Ok(())
}

#[test]
fn coordinator_default_textarea_wrap_mode_word_or_glyph() -> Result<()> {
    // -- Setup & Fixtures
    let coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec & Check
    assert_eq!(
        coordinator.textarea.wrap_mode(),
        ratatui_textarea::WrapMode::WordOrGlyph,
        "coordinator default textarea must enable WordOrGlyph soft wrap"
    );
    Ok(())
}

#[tokio::test]
async fn input_height_grows_with_wrapped_rows_while_typing() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: type a sentence that wraps at the narrow content width.
    let script: Vec<DriveEvent> = "hello world foo bar"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance(&script).await?;

    // -- Check: "hello world foo bar" wraps into 2 visual rows at width 14.
    assert_eq!(
        driver.coordinator().input_height,
        2,
        "input box must grow to the wrapped visual row count"
    );
    Ok(())
}

#[tokio::test]
async fn input_height_boundary_width_uses_crate_wrap_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: type a 15-char line that wraps at the crate's real width 14
    // (columns 20 − 2·side_margin − 4) but not at the 2-wider estimate 16.
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance(&script).await?;

    // -- Check: the crate wraps at 14 → 2 rows; the box must match.
    assert_eq!(
        driver.coordinator().input_height,
        2,
        "input box must wrap at the crate's real width (14), not the 2-wider estimate"
    );
    Ok(())
}

#[tokio::test]
async fn input_height_recomputes_on_resize() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    let script: Vec<DriveEvent> = "hello world foo bar baz qux"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance(&script).await?;

    // At the wide width the sentence fits on one row.
    assert_eq!(driver.coordinator().input_height, 1);

    // -- Exec: resize to a narrow width so the text wraps.
    driver
        .advance(&[DriveEvent::Key(TerminalEvent::Resize(
            crate::interaction::input::TerminalResize {
                columns: 20,
                rows: 30,
            },
        ))])
        .await?;

    // -- Check: the box height must grow to the wrapped row count.
    assert!(
        driver.coordinator().input_height > 1,
        "resize must recompute input_height from the new content width"
    );
    Ok(())
}

#[tokio::test]
async fn screen_cursor_wrapped_line_positions_terminal_cursor_on_visual_row() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: type a line that wraps to 2 visual rows at content width 14.
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance_with_frame(&script).await?;

    // -- Check: the caret sits on the second visual row; the terminal cursor
    // must render on the mode-indicator row + 1.
    let buffer = driver.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    let pos = driver.cursor_position();
    assert_eq!(
        pos.y,
        indicator_y + 1,
        "wrapped caret must render on the second visual row"
    );
    Ok(())
}

#[tokio::test]
async fn alt_return_cursor_tracks_new_logical_line_after_wrapped_first_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: type a wrapping line, then insert a newline with AltEnter.
    let mut script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    script.push(key(TerminalKey::AltEnter));
    driver.advance_with_frame(&script).await?;

    // -- Check: the new logical line starts at visual row 2 (the first line
    // wraps to rows 0-1); the terminal cursor must render on indicator + 2.
    let buffer = driver.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    let pos = driver.cursor_position();
    assert_eq!(
        pos.y,
        indicator_y + 2,
        "alt-return caret must land on the first visual row of the new line"
    );
    Ok(())
}

#[tokio::test]
async fn screen_cursor_internal_scroll_positions_cursor_on_last_visible_row() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: type a long single word that wraps to >6 visual rows.
    let script: Vec<DriveEvent> = "x"
        .repeat(100)
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance_with_frame(&script).await?;

    // -- Check: with internal scroll the caret is on the last visible row
    // (visual row 5 of 6); the terminal cursor must render on indicator + 5.
    let buffer = driver.buffer_text();
    let indicator_y = buffer
        .lines()
        .position(|l| l.contains("❯"))
        .ok_or("should find mode indicator row")? as u16;
    let pos = driver.cursor_position();
    assert_eq!(
        pos.y,
        indicator_y + 5,
        "scrolled caret must render on the last visible input row"
    );
    Ok(())
}

#[tokio::test]
async fn history_up_mid_wrapped_first_line_moves_cursor_not_history() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("prev".to_string());
    let _ = driver.coordinator_mut().state.activate_next_prompt();
    driver.coordinator_mut().state.complete_active_prompt();

    // Type a wrapping line; the caret ends on the second visual row.
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance(&script).await?;
    assert_eq!(driver.coordinator_mut().textarea.screen_cursor().row, 1);

    // -- Exec: press Up while mid-way through the wrapped first line.
    driver.advance(&[key(TerminalKey::Up)]).await?;

    // -- Check: history must NOT be loaded; the cursor moves within the
    // textarea to the first visual row.
    assert_eq!(
        driver.coordinator_mut().textarea.lines().join("\n"),
        "hello world foo",
        "Up mid-wrap must not load history"
    );
    assert_eq!(
        driver.coordinator_mut().textarea.screen_cursor().row,
        0,
        "Up mid-wrap must move the cursor up within the textarea"
    );
    Ok(())
}

#[tokio::test]
async fn history_down_last_visual_row_loads_history() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("prev".to_string());
    let _ = driver.coordinator_mut().state.activate_next_prompt();
    driver.coordinator_mut().state.complete_active_prompt();

    // Type a wrapping line; the caret is on the last visual row.
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance(&script).await?;
    assert_eq!(driver.coordinator_mut().textarea.screen_cursor().row, 1);

    // -- Exec: press Up twice to enter history navigation (Up on row 1 moves
    // within the textarea to row 0; Up on row 0 loads history), then Down on
    // the last visual row of the loaded history item.
    driver
        .advance(&[key(TerminalKey::Up), key(TerminalKey::Up)])
        .await?;
    assert_eq!(
        driver.coordinator_mut().textarea.lines().join("\n"),
        "prev",
        "Up on the first visual row must load history"
    );
    driver.advance(&[key(TerminalKey::Down)]).await?;

    // -- Check: Down on the last visual row must restore the saved draft.
    assert_eq!(
        driver.coordinator_mut().textarea.lines().join("\n"),
        "hello world foo",
        "Down on the last visual row must restore the saved draft"
    );
    Ok(())
}

#[tokio::test]
async fn history_up_first_visual_row_loads_history() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("prev".to_string());
    let _ = driver.coordinator_mut().state.activate_next_prompt();
    driver.coordinator_mut().state.complete_active_prompt();

    // Type a short line that fits on one row; the caret is on the first
    // visual row.
    let script: Vec<DriveEvent> = "hi".chars().map(|c| key(TerminalKey::Char(c))).collect();
    driver.advance(&script).await?;
    assert_eq!(driver.coordinator_mut().textarea.screen_cursor().row, 0);

    // -- Exec: press Up on the first visual row.
    driver.advance(&[key(TerminalKey::Up)]).await?;

    // -- Check: history must be loaded.
    assert_eq!(
        driver.coordinator_mut().textarea.lines().join("\n"),
        "prev",
        "Up on the first visual row must load history"
    );
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
