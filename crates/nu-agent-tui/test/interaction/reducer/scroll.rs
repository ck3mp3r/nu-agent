use super::*;
use crate::state::selection::TranscriptSelection;

#[test]
fn normal_j_moves_cursor_not_viewport() {
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
    // Cursor moved from 5 to 6, viewport didn't scroll (6 < 0+10-3=7)
    assert_eq!(state.scroll.cursor_visual_row, 6);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);
}

#[test]
fn normal_j_scrolls_viewport_when_cursor_leaves_margin() {
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
    // scroll_margin = 3, viewport_bottom = 0+10-3 = 7
    // cursor moves 6→7, visual_row=7 >= 7, viewport scrolls
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineDown),
        None,
    );
    assert_eq!(state.scroll.cursor_visual_row, 7);
    assert_eq!(state.scroll.scroll_offset, 1);
    assert!(!state.scroll.following_tail);
}

#[test]
fn visual_j_scrolls_viewport_only_when_cursor_leaves_visible_area() {
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
    // 20 entries, 1 visual row each
    for i in 0..20 {
        push_user_line(&mut state, format!("line {i}"));
    }
    // scroll_margin = 10/3 = 3
    // viewport shows rows 0-9 (scroll_offset=0, viewport_height=10)
    // Press j 7 times: cursor moves 0→1→2→3→4→5→6→7
    // Cursor 1-6: visual row < 0+10-3=7, no scroll
    // Cursor 7: visual row 7 >= 7, scroll_offset → 1
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
}

#[test]
fn visual_k_syncs_scroll_offset_when_exiting_tail_following() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            following_tail: true,
            max_scroll: 50,
            scroll_offset: 0, // stale
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
    // scroll_offset should be synced from max_scroll (50), then cursor moves
    // 55→54. cursor_visual_row=54, scroll_margin=1, 54 < 50+1=51 is false,
    // so no additional scroll.
    assert_eq!(state.scroll.cursor_visual_row, 54);
    assert_eq!(state.scroll.scroll_offset, 50);
    assert!(!state.scroll.following_tail);
}

#[test]
fn ctrl_u_moves_cursor_up_by_page() {
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
    // cursor moves 10 - 8 = 2
    assert_eq!(state.scroll.cursor_visual_row, 2);
    // scroll_margin = 10/3 = 3, cursor 2 < 5 + 3 = 8, so scroll_offset = 5 - 8 = 0
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);
}

#[test]
fn ctrl_d_moves_cursor_down_by_page() {
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
    // cursor moves 0 + 8 = 8
    assert_eq!(state.scroll.cursor_visual_row, 8);
    // scroll_margin = 3, viewport_bottom = 0 + 10 - 3 = 7, cursor 8 >= 7, so scroll_offset = 0 + 8 = 8
    assert_eq!(state.scroll.scroll_offset, 8);
    assert!(!state.scroll.following_tail);
}

#[test]
fn ctrl_u_in_visual_mode_extends_selection() {
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
}

#[test]
fn ctrl_d_in_visual_mode_extends_selection() {
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
