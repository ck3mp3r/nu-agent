use super::*;
use ratatui::layout::Rect;

#[test]
fn mode_transition_table() {
    // Enter insert and normal mode actions toggle mode only in idle.
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    state.enter_normal_mode();
    assert_eq!(state.input.mode, InputMode::Normal);

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterInsertMode),
        None,
    );
    assert_eq!(state.input.mode, InputMode::Insert);

    // Enter normal mode from chord switches mode.
    let mut state = AppState::default();

    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterNormalModeFromChord),
        None,
    );

    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn test_wheel_in_insert_mode_activates_normal() -> Result<()> {
    // -- Setup & Fixtures: idle insert mode with pending input text.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("draft".to_string()),
        scroll: ScrollState {
            viewport_height: 10,
            cursor_visual_row: 0,
            total_visual_rows: 30,
            entry_indices: (0..30).collect(),
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
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 5, row: 5 }),
        None,
    );

    // -- Check: normal mode, transcript focus, input text preserved.
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Transcript);
    assert_eq!(state.input.pending_submit_text.as_deref(), Some("draft"));
    assert_eq!(state.scroll.cursor_visual_row, 3);

    // -- Setup & Fixtures: busy phase in insert mode.
    let mut busy = busy_state_with_clean_transcript();
    busy.scroll.viewport_height = 10;
    busy.scroll.total_visual_rows = 30;
    busy.scroll.entry_indices = (0..30).collect();
    busy.scroll.transcript_area = Rect::new(0, 0, 80, 20);
    assert_eq!(busy.input.mode, InputMode::Insert);
    assert_eq!(busy.phase, UiPhase::Busy);

    // -- Exec
    reduce_with_cancel_controller(
        &mut busy,
        ReducerInput::User(UserAction::WheelUp { column: 5, row: 5 }),
        None,
    );

    // -- Check: the busy phase also activates normal mode.
    assert_eq!(busy.input.mode, InputMode::Normal);
    assert_eq!(busy.scroll.pane_focus, PaneFocus::Transcript);
    Ok(())
}
