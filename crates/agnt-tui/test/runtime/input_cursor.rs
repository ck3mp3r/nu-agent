use super::*;

use crate::rendering::layout::input_content_row_count;

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
/// only on an empty input's single visual row, switches immediately on a mode
/// change, and the content width subtracts only the 2-char prompt prefix.
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
    assert_eq!(input_rows_with_prompt_for_test(&insert), vec!["❯ "]);
    assert_eq!(input_rows_with_prompt_for_test(&normal), vec!["❮ "]);
    assert_eq!(input_rows_with_prompt_for_test(&visual), vec!["❮ "]);

    // -- Exec & Check: an empty input paints the indicator alone.
    let rows = input_rows_with_prompt_for_test(&insert);
    assert_eq!(rows, vec!["❯ "]);

    // -- Exec & Check: the prefix switches immediately on a mode change.
    let mut switching = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };
    assert_eq!(input_rows_with_prompt_for_test(&switching), vec!["❯ "]);
    switching.input.mode = InputMode::Normal;
    assert_eq!(input_rows_with_prompt_for_test(&switching), vec!["❮ "]);

    // -- Exec & Check: the content width subtracts only the prompt prefix, and
    // the textarea wraps at that width.
    let inner_width: u16 = 10;
    let content_width = input_pane_content_width_for_test(inner_width);
    assert_eq!(content_width, 8);
    let textarea = RuntimeCoordinator::new_textarea(vec!["abcdefghi".to_string()]);
    assert_eq!(input_content_row_count(&textarea, content_width as u16), 2);
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
