use super::*;
use crate::state::selection::TranscriptSelection;

#[test]
fn enter_visual_mode_selects_first_visible_entry_not_scroll_offset() {
    let mut state = AppState {
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            ..Default::default()
        },
        ..Default::default()
    };
    // Populate transcript with entries that render as multiple lines
    // Entry 0: multi-line markdown (renders as 3 visual rows)
    // Entry 1: multi-line markdown (renders as 2 visual rows)
    push_user_line(&mut state, "line 0\nextra\nmore");
    push_user_line(&mut state, "line 1\nextra");
    // Scroll offset = 2 means we've scrolled past entry 0's 3 lines
    // (offset 0, 1, 2 are all entry 0's visual rows)
    // The first visible entry should be entry 1
    state.scroll.scroll_offset = 2;
    state.scroll.cursor_visual_row = 3;
    state.scroll.entry_indices = vec![0, 0, 0, 1, 1];
    state.scroll.total_visual_rows = 5;
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterVisualMode),
        None,
    );
    let sel = state.scroll.selection.expect("selection should be set");
    // Should select visual row 3 (maps to entry 1 via entry_indices)
    assert_eq!(sel.anchor(), 3);
    assert_eq!(sel.cursor(), 3);
    assert_eq!(state.input.mode, InputMode::Visual);
}

#[test]
fn enter_visual_mode_sets_selection_at_scroll_offset() {
    let mut state = AppState {
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            cursor_visual_row: 5,
            entry_indices: (0..10).collect(),
            total_visual_rows: 10,
            ..Default::default()
        },
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterVisualMode),
        None,
    );
    let sel = state.scroll.selection.expect("selection should be set");
    assert_eq!(sel.anchor(), 5);
    assert_eq!(sel.cursor(), 5);
    assert_eq!(state.input.mode, InputMode::Visual);
    assert!(state.status.message.status_line().is_empty());
}

#[test]
fn enter_visual_mode_requires_transcript_focus() {
    let mut state = AppState {
        scroll: ScrollState {
            pane_focus: PaneFocus::Input,
            ..Default::default()
        },
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterVisualMode),
        None,
    );
    assert_eq!(
        state.status.message.status_line(),
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS
    );
    assert!(state.scroll.selection.is_none());
}

#[test]
fn enter_visual_mode_noop_when_busy() {
    let mut state = AppState {
        phase: UiPhase::Busy,
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::EnterVisualMode),
        None,
    );
    assert!(state.scroll.selection.is_none());
}

#[test]
fn visual_j_extends_selection_down() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            selection: Some(TranscriptSelection::new(0)),
            entry_indices: (0..5).collect(),
            total_visual_rows: 5,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..5 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineDown),
        None,
    );
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(sel.cursor(), 1);
    assert_eq!(state.scroll.cursor_visual_row, 1);
    assert_eq!(state.scroll.scroll_offset, 1);
    assert!(!state.scroll.following_tail);
}

#[test]
fn visual_k_extends_selection_up() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            selection: Some(TranscriptSelection::new(2)),
            cursor_visual_row: 2,
            entry_indices: (0..3).collect(),
            total_visual_rows: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollLineUp),
        None,
    );
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(sel.cursor(), 1);
    assert_eq!(state.scroll.cursor_visual_row, 1);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);
}

#[test]
fn visual_yank_copies_and_exits_visual() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            rendered_line_text: vec![
                "line 0".to_string(),
                "line 1".to_string(),
                "line 2".to_string(),
                "line 3".to_string(),
                "line 4".to_string(),
            ],
            rendered_line_start_row: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    // Set selection covering visual rows 1-3
    let mut sel = TranscriptSelection::new(1);
    sel.set_cursor(3);
    state.scroll.selection = Some(sel);
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::YankSelection),
        None,
    );
    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("line 1\nline 2\nline 3".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn visual_yank_copies_only_selected_rows_not_whole_entry() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            rendered_line_text: (0..10).map(|i| format!("row {i}")).collect(),
            rendered_line_start_row: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    // Select rows 3-5 out of 10
    let mut sel = TranscriptSelection::new(3);
    sel.set_cursor(5);
    state.scroll.selection = Some(sel);
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::YankSelection),
        None,
    );
    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("row 3\nrow 4\nrow 5".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn visual_yank_with_nonzero_scroll_offset_copies_correct_rows() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            // Simulate viewport scrolled down — rendered_line_text[0] is absolute row 10
            rendered_line_start_row: 10,
            rendered_line_text: vec![
                "row 10".to_string(),
                "row 11".to_string(),
                "row 12".to_string(),
                "row 13".to_string(),
                "row 14".to_string(),
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    // Select absolute visual rows 11-13
    let mut sel = TranscriptSelection::new(11);
    sel.set_cursor(13);
    state.scroll.selection = Some(sel);
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::YankSelection),
        None,
    );
    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("row 11\nrow 12\nrow 13".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn visual_yank_nothing_to_yank_leaves_status_empty() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            rendered_line_text: Vec::new(),
            scroll_offset: 0,
            selection: Some(TranscriptSelection::new(0)),
            ..Default::default()
        },
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::YankSelection),
        None,
    );
    assert!(state.input.take_clipboard_request().is_none());
    assert!(state.status.message.status_line().is_empty());
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn visual_yank_empty_selection_noop() {
    let mut state = AppState::default();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::YankSelection),
        None,
    );
    assert!(state.input.take_clipboard_request().is_none());
}

#[test]
fn visual_esc_clears_selection_and_exits() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            selection: Some(TranscriptSelection::new(0)),
            ..Default::default()
        },
        ..Default::default()
    };
    reduce_with_cancel_controller(&mut state, ReducerInput::User(UserAction::Esc), None);
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn visual_gg_jumps_to_top() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            selection: Some(TranscriptSelection::new(5)),
            ..Default::default()
        },
        ..Default::default()
    };
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollToTop),
        None,
    );
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(sel.cursor(), 0);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);
}

#[test]
fn visual_g_jumps_to_bottom() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            selection: Some(TranscriptSelection::new(0)),
            total_visual_rows: 5,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..5 {
        push_user_line(&mut state, format!("line {i}"));
    }
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::ScrollToBottom),
        None,
    );
    let sel = state.scroll.selection.expect("selection should exist");
    assert_eq!(sel.cursor(), 4);
    assert!(state.scroll.following_tail);
}
