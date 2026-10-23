use super::*;
use crate::state::selection::TranscriptSelection;
use ratatui::layout::Rect;

#[test]
fn scroll_line_cursor_and_viewport_table() {
    // Normal j moves the cursor, not the viewport.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 5,
            entry_indices: (0..20).collect(),
            total_visual_rows: 20,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..20 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineDown),
        None,
    );
    assert_eq!(state.scroll.cursor_visual_row, 6);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);

    // Normal j scrolls the viewport when the cursor leaves the margin.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 6,
            entry_indices: (0..20).collect(),
            total_visual_rows: 20,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..20 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineDown),
        None,
    );
    assert_eq!(state.scroll.cursor_visual_row, 7);
    assert_eq!(state.scroll.scroll_offset, 1);
    assert!(!state.scroll.following_tail);

    // Visual j scrolls the viewport only when the cursor leaves the visible area.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            following_tail: false,
            viewport_height: 10,
            scroll_offset: 0,
            selection: Some(TranscriptSelection::new(0)),
            entry_indices: (0..20).collect(),
            total_visual_rows: 20,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..20 {
        push_user_line(&mut state, format!("line {i}"));
    }
    for _ in 0..7 {
        reduce_with_cancel_controller(
            &mut state,
            ReducerInput::User(UserAction::ScrollLineDown),
            None,
        );
    }
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(sel.cursor(), 7);
    assert_eq!(state.scroll.cursor_visual_row, 7);
    assert_eq!(state.scroll.scroll_offset, 1);
    assert!(!state.scroll.following_tail);

    // Visual k syncs the scroll offset when exiting tail following.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            following_tail: true,
            max_scroll: 50,
            scroll_offset: 0,
            cursor_visual_row: 55,
            selection: Some(TranscriptSelection::new(55)),
            entry_indices: (0..60).collect(),
            total_visual_rows: 60,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..60 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineUp),
        None,
    );
    assert_eq!(state.scroll.cursor_visual_row, 54);
    assert_eq!(state.scroll.scroll_offset, 50);
    assert!(!state.scroll.following_tail);
}

#[test]
fn scroll_page_and_selection_table() {
    // Ctrl-U moves the cursor up by a page.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 10,
            total_visual_rows: 30,
            scroll_offset: 5,
            following_tail: false,
            entry_indices: (0..30).collect(),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollPageUp),
        None,
    );
    assert_eq!(state.scroll.cursor_visual_row, 2);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);

    // Ctrl-D moves the cursor down by a page.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 0,
            total_visual_rows: 30,
            scroll_offset: 0,
            following_tail: false,
            entry_indices: (0..30).collect(),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollPageDown),
        None,
    );
    assert_eq!(state.scroll.cursor_visual_row, 8);
    assert_eq!(state.scroll.scroll_offset, 8);
    assert!(!state.scroll.following_tail);

    // Ctrl-U in visual mode extends the selection.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            selection: Some(TranscriptSelection::new(10)),
            cursor_visual_row: 10,
            total_visual_rows: 30,
            viewport_height: 10,
            scroll_offset: 5,
            following_tail: false,
            entry_indices: (0..30).collect(),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollPageUp),
        None,
    );
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(state.scroll.cursor_visual_row, 2);
    assert_eq!(sel.cursor(), 2);

    // Ctrl-D in visual mode extends the selection.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            selection: Some(TranscriptSelection::new(0)),
            cursor_visual_row: 0,
            total_visual_rows: 30,
            viewport_height: 10,
            scroll_offset: 0,
            following_tail: false,
            entry_indices: (0..30).collect(),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollPageDown),
        None,
    );
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(state.scroll.cursor_visual_row, 8);
    assert_eq!(sel.cursor(), 8);
}

#[test]
fn test_wheel_scroll_three_lines_per_notch_and_tail_resume() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 0,
            total_visual_rows: 30,
            scroll_offset: 0,
            following_tail: false,
            entry_indices: (0..30).collect(),
            transcript_area: Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }

    // -- Exec
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 5, row: 5 }),
        None,
    );

    // -- Check: three visual lines down, the tail is not resumed yet.
    assert!(changed);
    assert_eq!(state.scroll.cursor_visual_row, 3);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);

    // -- Exec
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelUp { column: 5, row: 5 }),
        None,
    );

    // -- Check: three visual lines up.
    assert_eq!(state.scroll.cursor_visual_row, 0);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);

    // -- Setup & Fixtures: the cursor three rows above the last visual row.
    state.scroll.cursor_visual_row = 27;
    state.scroll.scroll_offset = 20;
    state.scroll.max_scroll = 20;
    state.scroll.following_tail = false;

    // -- Exec
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 5, row: 5 }),
        None,
    );

    // -- Check: the cursor lands on the last visual row and the tail resumes.
    assert_eq!(state.scroll.cursor_visual_row, 29);
    assert!(state.scroll.following_tail);
    Ok(())
}

#[test]
fn test_wheel_scroll_hit_test_rejects_outside_transcript_area() -> Result<()> {
    // -- Setup & Fixtures: no render yet, so the stored area is empty.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 5,
            total_visual_rows: 30,
            entry_indices: (0..30).collect(),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }

    // -- Exec
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 5, row: 5 }),
        None,
    );

    // -- Check: nothing moves before the first render.
    assert!(!changed);
    assert_eq!(state.scroll.cursor_visual_row, 5);
    assert_eq!(state.scroll.scroll_offset, 0);

    // -- Setup & Fixtures: a rendered transcript area.
    state.scroll.transcript_area = Rect::new(10, 2, 40, 10);

    // -- Exec: a point left of the area.
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 9, row: 5 }),
        None,
    );

    // -- Check
    assert!(!changed);
    assert_eq!(state.scroll.cursor_visual_row, 5);

    // -- Exec: the area's top-left cell.
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 10, row: 2 }),
        None,
    );

    // -- Check
    assert!(changed);
    assert_eq!(state.scroll.cursor_visual_row, 8);
    Ok(())
}

#[test]
fn test_wheel_scroll_extends_visual_selection() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 10,
            total_visual_rows: 30,
            scroll_offset: 5,
            following_tail: false,
            selection: Some(TranscriptSelection::new(10)),
            entry_indices: (0..30).collect(),
            transcript_area: Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..30 {
        push_user_line(&mut state, format!("line {i}"));
    }

    // -- Exec
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelDown { column: 5, row: 5 }),
        None,
    );

    // -- Check: the selection cursor follows the wheel.
    let sel = state.scroll.selection.ok_or("selection should exist")?;
    assert_eq!(state.scroll.cursor_visual_row, 13);
    assert_eq!(sel.cursor(), 13);

    // -- Exec
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelUp { column: 5, row: 5 }),
        None,
    );

    // -- Check
    let sel = state.scroll.selection.ok_or("selection should exist")?;
    assert_eq!(state.scroll.cursor_visual_row, 10);
    assert_eq!(sel.cursor(), 10);
    Ok(())
}

#[test]
fn test_wheel_scroll_insert_mode_noop_reports_mode_change() -> Result<()> {
    // -- Setup & Fixtures: insert mode, cursor at the top, nothing to scroll.
    let mut state = wheel_noop_state(InputMode::Insert);

    // -- Exec
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelUp { column: 5, row: 5 }),
        None,
    );

    // -- Check: the scroll is a no-op, but the mode transition is visible.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Transcript);
    assert_eq!(state.scroll.cursor_visual_row, 0);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);
    Ok(())
}

#[test]
fn test_wheel_scroll_normal_mode_noop_reports_no_change() -> Result<()> {
    // -- Setup & Fixtures: normal mode, cursor at the top, nothing to scroll.
    let mut state = wheel_noop_state(InputMode::Normal);

    // -- Exec
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelUp { column: 5, row: 5 }),
        None,
    );

    // -- Check: no mode transition and no scroll change.
    assert!(!changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.scroll.cursor_visual_row, 0);
    assert_eq!(state.scroll.scroll_offset, 0);
    Ok(())
}

#[test]
fn test_wheel_scroll_insert_mode_scroll_change_reports_change() -> Result<()> {
    // -- Setup & Fixtures: insert mode with room to scroll up.
    let mut state = wheel_noop_state(InputMode::Insert);
    state.scroll.cursor_visual_row = 10;
    state.scroll.scroll_offset = 5;
    state.scroll.max_scroll = 20;

    // -- Exec
    let changed = reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::WheelUp { column: 5, row: 5 }),
        None,
    );

    // -- Check: the scroll moved and the mode transitioned.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.scroll.cursor_visual_row, 7);
    assert_eq!(state.scroll.scroll_offset, 4);
    Ok(())
}
