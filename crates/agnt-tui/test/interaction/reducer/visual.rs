use super::*;
use crate::state::selection::TranscriptSelection;

#[test]
fn visual_mode_entry_table() -> Result<()> {
    // Selects the first visible entry, not the scroll offset.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
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
    state.scroll.total_visual_rows = 5;
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('v'),
        },
        None,
    );
    let sel = state.scroll.selection.ok_or("selection should be set")?;
    assert_eq!(sel.anchor(), 3);
    assert_eq!(sel.cursor(), 3);
    assert_eq!(state.input.mode, InputMode::Visual);

    // Sets the selection at the scroll offset.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            cursor_visual_row: 5,
            total_visual_rows: 10,
            ..Default::default()
        },
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('v'),
        },
        None,
    );
    let sel = state.scroll.selection.ok_or("selection should be set")?;
    assert_eq!(sel.anchor(), 5);
    assert_eq!(sel.cursor(), 5);
    assert_eq!(state.input.mode, InputMode::Visual);
    assert!(state.status.message.status_line().is_empty());

    // Requires transcript focus.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Input,
            ..Default::default()
        },
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('v'),
        },
        None,
    );
    assert_eq!(
        state.status.message.status_line(),
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS
    );
    assert!(state.scroll.selection.is_none());

    // No-op when busy.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        phase: UiPhase::Busy,
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('v'),
        },
        None,
    );
    assert!(state.scroll.selection.is_none());
    Ok(())
}

#[test]
fn visual_mode_extension_table() -> Result<()> {
    // Visual j extends the selection down.
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
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );
    let sel = state.scroll.selection.ok_or("selection should exist")?;
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
            total_visual_rows: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('k'),
        },
        None,
    );
    let sel = state.scroll.selection.ok_or("selection should exist")?;
    assert_eq!(sel.cursor(), 1);
    assert_eq!(state.scroll.cursor_visual_row, 1);
    assert_eq!(state.scroll.scroll_offset, 0);
    assert!(!state.scroll.following_tail);
    Ok(())
}

#[test]
fn visual_yank_table() {
    // Yank copies the selected rows and exits visual mode. Three user blocks
    // render as: row 0 = "line 0", rows 1-2 = separators, row 3 = "line 1",
    // rows 4-5 = separators, row 6 = "line 2", row 7 = trailing separator.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            transcript_area: ratatui::layout::Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..3 {
        push_user_line(&mut state, format!("line {i}"));
    }
    state.transcript.rebuild_height_index(78);
    state.scroll.selection = Some(TranscriptSelection::new(0));
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('y'),
        },
        None,
    );
    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("╎ line 0".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);

    // Yank copies only the selected rows, not the whole entry.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            transcript_area: ratatui::layout::Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..3 {
        push_user_line(&mut state, format!("line {i}"));
    }
    state.transcript.rebuild_height_index(78);
    state.scroll.selection = Some(TranscriptSelection::new(3));
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('y'),
        },
        None,
    );
    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("╎ line 1".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);

    // Yank with a nonzero scroll offset copies the correct rows: the payload
    // follows the absolute row range, not the viewport position.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            transcript_area: ratatui::layout::Rect::new(0, 0, 80, 20),
            viewport_height: 2,
            scroll_offset: 2,
            following_tail: false,
            ..Default::default()
        },
        ..Default::default()
    };
    for i in 0..3 {
        push_user_line(&mut state, format!("line {i}"));
    }
    state.transcript.rebuild_height_index(78);
    let mut sel = TranscriptSelection::new(3);
    sel.set_cursor(4);
    state.scroll.selection = Some(sel);
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('y'),
        },
        None,
    );
    let clipboard = state.input.take_clipboard_request();
    // Row 4 is the bleed separator row below the user block, which carries the
    // user rail (task 4d99956c), so the payload keeps the rail glyph.
    assert_eq!(clipboard, Some("╎ line 1\n╎".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);

    // Nothing to yank leaves the status empty.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            transcript_area: ratatui::layout::Rect::new(0, 0, 80, 20),
            selection: Some(TranscriptSelection::new(0)),
            ..Default::default()
        },
        ..Default::default()
    };
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('y'),
        },
        None,
    );
    assert!(state.input.take_clipboard_request().is_none());
    assert!(state.status.message.status_line().is_empty());
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);

    // An empty selection is a no-op.
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('y'),
        },
        None,
    );
    assert!(state.input.take_clipboard_request().is_none());
}

#[test]
fn visual_mode_exit_and_jump_table() -> Result<()> {
    // Esc clears the selection and exits.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        scroll: ScrollState {
            selection: Some(TranscriptSelection::new(0)),
            ..Default::default()
        },
        ..Default::default()
    };
    reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
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
    for _ in 0..2 {
        reduce(
            &mut state,
            SystemEvent::KeyPressed {
                key: Key::Char('g'),
            },
            None,
        );
    }
    let sel = state.scroll.selection.ok_or("selection should exist")?;
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
    reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('G'),
        },
        None,
    );
    let sel = state.scroll.selection.ok_or("selection should exist")?;
    assert_eq!(sel.cursor(), 4);
    assert!(state.scroll.following_tail);
    Ok(())
}
