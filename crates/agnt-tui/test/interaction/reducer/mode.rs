use super::*;
use ratatui::layout::Rect;

#[test]
fn mode_transition_table() {
    // `i` enters insert mode in normal mode.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    assert_eq!(state.input.mode, InputMode::Normal);

    let changed = reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('i'),
        },
        None,
    );
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Insert);

    // The insert-exit chord leaves insert mode for normal mode.
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );
    assert_eq!(
        state.input.mode,
        InputMode::Insert,
        "the chord is only armed"
    );

    reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn test_wheel_in_insert_mode_keeps_insert_mode() -> Result<()> {
    // -- Setup & Fixtures: idle insert mode with pending input text.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("draft".to_string()),
        scroll: ScrollState {
            viewport_height: 10,
            cursor_visual_row: 0,
            total_visual_rows: 30,
            transcript_area: Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);

    // -- Exec
    reduce(
        &mut state,
        UiInput::WheelScrolled {
            up: false,
            column: 5,
            row: 5,
        },
        None,
    );

    // -- Check: insert mode and the composer keep their ground; the notch is a
    // transient gesture and the draft survives it.
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);
    assert_eq!(state.input.pending_submit_text.as_deref(), Some("draft"));
    assert_eq!(state.scroll.cursor_visual_row, 3);

    // -- Setup & Fixtures: busy phase in insert mode.
    let mut busy = busy_state_with_clean_transcript();
    busy.scroll.viewport_height = 10;
    busy.scroll.total_visual_rows = 30;
    busy.scroll.transcript_area = Rect::new(0, 0, 80, 20);
    assert_eq!(busy.input.mode, InputMode::Insert);
    assert_eq!(busy.phase, UiPhase::Busy);

    // -- Exec
    reduce(
        &mut busy,
        UiInput::WheelScrolled {
            up: true,
            column: 5,
            row: 5,
        },
        None,
    );

    // -- Check: the busy phase keeps insert mode and the composer too.
    assert_eq!(busy.input.mode, InputMode::Insert);
    assert_eq!(busy.scroll.pane_focus, PaneFocus::Input);
    Ok(())
}
