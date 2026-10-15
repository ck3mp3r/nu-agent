use super::*;
use crate::state::selection::TranscriptSelection;

#[test]
fn visual_mode_entry_table() {
    // Selects the first visible entry, not the scroll offset.
    let mut state = AppState {
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            ..Default::default()
        },
        ..Default::default()
    };
    push_user_line(&mut state, "line 0\nextra\nmore");
    push_user_line(&mut state, "line 1\nextra");
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
    assert_eq!(sel.anchor(), 3);
    assert_eq!(sel.cursor(), 3);
    assert_eq!(state.input.mode, InputMode::Visual);

    // Sets the selection at the scroll offset.
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

    // Requires transcript focus.
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

    // No-op when busy.
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
fn visual_mode_extension_table() {
    // Visual j extends the selection down.
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

    // Visual k extends the selection up.
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
fn visual_yank_table() {
    // Yank copies and exits visual mode.
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

    // Yank copies only the selected rows, not the whole entry.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            rendered_line_text: (0..10).map(|i| format!("row {i}")).collect(),
            rendered_line_start_row: 0,
            ..Default::default()
        },
        ..Default::default()
    };
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

    // Yank with a nonzero scroll offset copies the correct rows.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
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

    // Nothing to yank leaves the status empty.
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

    // An empty selection is a no-op.
    let mut state = AppState::default();
    reduce_with_cancel_controller(
        &mut state,
        ReducerInput::User(UserAction::YankSelection),
        None,
    );
    assert!(state.input.take_clipboard_request().is_none());
}

#[test]
fn visual_mode_exit_and_jump_table() {
    // Esc clears the selection and exits.
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

    // gg jumps to the top.
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

    // g jumps to the bottom.
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
