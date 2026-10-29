use super::*;
use crate::state::selection::TranscriptSelection;
use agnt_core::transcript::ir::{Block, MessageRole};
use agnt_core::transcript::items::Message;
use agnt_core::transcript::renderer::Renderable;

// region:    --- Test Support

/// Push `count` one-row assistant prose blocks into the transcript.
fn push_assistant_blocks(state: &mut AppState, count: usize) {
    for i in 1..=count {
        let msg = Message {
            role: MessageRole::Assistant,
            markdown: format!("assistant text {i}"),
        };
        state.transcript.push_block(Block {
            source: msg.source(),
            lane: msg.lane(),
            fill: msg.fill(),
            status: None,
        });
    }
}

/// Point the scroll state at an 80-column transcript pane and build the height
/// index the yank path reads.
fn sync_transcript_geometry(state: &mut AppState) {
    state.scroll.transcript_area = Rect::new(0, 0, 80, 20);
    state.transcript.rebuild_height_index(78);
    state.scroll.total_visual_rows = state.transcript.total_visual_rows();
}

// endregion: --- Test Support

#[test]
fn normal_v_key_enters_visual_mode_and_j_yank_works() -> Result<()> {
    // -- Setup & Fixtures: three one-row assistant blocks, 7 visual rows.
    let mut state = AppState::default();
    state.enter_normal_mode();
    push_assistant_blocks(&mut state, 3);
    sync_transcript_geometry(&mut state);
    state.scroll.cursor_visual_row = 0;

    // -- Exec: 'v' enters Visual mode and anchors the selection at the cursor.
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('v')),
        None,
    );
    assert_eq!(state.input.mode, InputMode::Visual);
    let sel = state
        .scroll
        .selection
        .ok_or("should have transcript selection after pressing v")?;
    assert_eq!(sel.anchor(), 0);
    assert_eq!(sel.cursor(), 0);

    // -- Exec: 'j' extends the selection down one visual row.
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );

    // -- Check
    let sel = state
        .scroll
        .selection
        .ok_or("should still have transcript selection after pressing j")?;
    assert_eq!(sel.cursor(), 1);

    // -- Exec: 'y' yanks the selected rows.
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('y')),
        None,
    );

    // -- Check: rows 0 and 1 are the first block's content row and the first
    // separator row above the second block.
    let clipboard = state.input.take_clipboard_request();
    assert_eq!(clipboard, Some("  assistant text 1\n".to_string()));
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}

#[test]
fn visual_yank_beyond_viewport_copies_every_selected_row() -> Result<()> {
    // -- Setup & Fixtures: six one-row assistant blocks separated by two blank
    // rows each — 16 visual rows in total.
    let mut state = AppState::default();
    state.enter_normal_mode();
    push_assistant_blocks(&mut state, 6);
    sync_transcript_geometry(&mut state);
    assert_eq!(state.transcript.total_visual_rows(), 16);
    // The viewport shows only the first four rows; the selection spans all 16.
    state.scroll.viewport_height = 4;
    state.scroll.following_tail = false;
    state.scroll.scroll_offset = 0;
    state.scroll.cursor_visual_row = 15;
    state.input.mode = InputMode::Visual;
    let mut sel = TranscriptSelection::new(0);
    sel.set_cursor(15);
    state.scroll.selection = Some(sel);

    // -- Exec
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('y')),
        None,
    );

    // -- Check: every selected row is copied, not only the viewport rows.
    let clipboard = state
        .input
        .take_clipboard_request()
        .ok_or("should have a clipboard request")?;
    let rows: Vec<&str> = clipboard.split('\n').collect();
    assert_eq!(
        rows.len(),
        16,
        "every selected visual row must be copied, got:\n{clipboard}"
    );
    assert_eq!(rows[0], "  assistant text 1");
    assert_eq!(rows[1], "");
    assert_eq!(rows[2], "");
    assert_eq!(rows[3], "  assistant text 2");
    assert_eq!(rows[15], "  assistant text 6");
    assert!(state.scroll.selection.is_none());
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}
