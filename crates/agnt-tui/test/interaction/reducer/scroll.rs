use super::*;
use crate::state::selection::TranscriptSelection;

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
