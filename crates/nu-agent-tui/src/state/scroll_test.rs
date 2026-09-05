//! Unit tests for [`ScrollState`]: viewport sync, scroll actions, selection
//! hand-off, and pane focus.

use super::*;
use crate::interaction::reducer::VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS;
use crate::state::selection::TranscriptSelection;

#[test]
fn test_scroll_state_defaults_follow_tail_with_input_focus() {
    // -- Setup & Fixtures
    // -- Exec
    let scroll = ScrollState::default();

    // -- Check
    assert_eq!(scroll.scroll_offset, 0);
    assert!(scroll.following_tail);
    assert_eq!(scroll.cursor_visual_row, 0);
    assert_eq!(scroll.viewport_height, 0);
    assert_eq!(scroll.max_scroll, 0);
    assert!(scroll.entry_indices.is_empty());
    assert_eq!(scroll.total_visual_rows, 0);
    assert!(scroll.rendered_line_text.is_empty());
    assert_eq!(scroll.rendered_line_start_row, 0);
    assert!(scroll.selection.is_none());
    assert!(scroll.entry_visual_info.is_empty());
    // The visual-info dirty flag moved with the transcript domain: a fresh
    // transcript store starts dirty so the renderer computes visual info.
    assert!(crate::state::TranscriptStore::default().visual_info_dirty);
    assert_eq!(scroll.pane_focus, PaneFocus::Input);
}

#[test]
fn test_scroll_state_sync_after_render_pins_cursor_while_tailing() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        viewport_height: 10,
        ..Default::default()
    };

    // -- Exec
    let max_scroll = scroll.sync_after_render(25);

    // -- Check
    assert_eq!(max_scroll, 15);
    assert_eq!(scroll.total_visual_rows, 25);
    assert_eq!(scroll.max_scroll, 15);
    assert_eq!(scroll.cursor_visual_row, 24, "cursor rides the last row");
}

#[test]
fn test_scroll_state_sync_after_render_leaves_cursor_when_not_tailing() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        viewport_height: 10,
        following_tail: false,
        cursor_visual_row: 4,
        ..Default::default()
    };

    // -- Exec
    let max_scroll = scroll.sync_after_render(25);

    // -- Check
    assert_eq!(max_scroll, 15);
    assert_eq!(scroll.cursor_visual_row, 4, "user-controlled cursor kept");
}

#[test]
fn test_scroll_state_sync_after_render_zero_total_rows_clamps() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        viewport_height: 10,
        following_tail: false,
        ..Default::default()
    };

    // -- Exec
    let max_scroll = scroll.sync_after_render(3);

    // -- Check
    assert_eq!(max_scroll, 0, "saturating_sub clamps at zero");
    assert_eq!(scroll.max_scroll, 0);
    assert_eq!(scroll.cursor_visual_row, 0);
}

#[test]
fn test_scroll_state_reduce_scroll_action_line_down_moves_cursor_and_offset() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        cursor_visual_row: 5,
        entry_indices: (0..20).collect(),
        total_visual_rows: 20,
        ..Default::default()
    };

    // -- Exec
    let changed = scroll.reduce_scroll_action(ScrollAction::LineDown { select: false });

    // -- Check
    assert!(changed);
    assert_eq!(scroll.cursor_visual_row, 6);
    assert_eq!(scroll.scroll_offset, 0, "viewport did not scroll");
}

#[test]
fn test_scroll_state_reduce_scroll_action_line_up_exits_tail_follow() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        max_scroll: 10,
        total_visual_rows: 20,
        ..Default::default()
    };
    assert!(scroll.following_tail);

    // -- Exec
    let changed = scroll.reduce_scroll_action(ScrollAction::LineUp { select: false });

    // -- Check
    assert!(changed);
    assert!(!scroll.following_tail);
    assert_eq!(
        scroll.scroll_offset, 9,
        "offset snaps to max_scroll, then the margin check nudges it"
    );
    assert_eq!(scroll.cursor_visual_row, 0, "cursor clamps at top");
}

#[test]
fn test_scroll_state_reduce_scroll_action_line_down_extends_selection() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        selection: Some(TranscriptSelection::new(0)),
        entry_indices: (0..5).collect(),
        total_visual_rows: 5,
        ..Default::default()
    };

    // -- Exec
    let changed = scroll.reduce_scroll_action(ScrollAction::LineDown { select: true });

    // -- Check
    assert!(changed);
    let sel = scroll
        .selection
        .as_ref()
        .ok_or("should still have selection")
        .unwrap();
    assert_eq!(sel.cursor(), 1);
    assert_eq!(sel.anchor(), 0);
}

#[test]
fn test_scroll_state_reduce_scroll_action_line_down_without_select_keeps_selection() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        selection: Some(TranscriptSelection::new(0)),
        entry_indices: (0..5).collect(),
        total_visual_rows: 5,
        ..Default::default()
    };

    // -- Exec
    let changed = scroll.reduce_scroll_action(ScrollAction::LineDown { select: false });

    // -- Check
    assert!(changed);
    let sel = scroll
        .selection
        .as_ref()
        .ok_or("should still have selection")
        .unwrap();
    assert_eq!(sel.cursor(), 0, "selection cursor untouched");
}

#[test]
fn test_scroll_state_reduce_scroll_action_to_top_resets_offset_and_selection() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        total_visual_rows: 20,
        selection: Some(TranscriptSelection::new(2)),
        ..Default::default()
    };

    // -- Exec
    let changed = scroll.reduce_scroll_action(ScrollAction::ToTop { select: true });

    // -- Check
    assert!(changed);
    assert_eq!(scroll.cursor_visual_row, 0);
    assert_eq!(scroll.scroll_offset, 0);
    assert!(!scroll.following_tail);
    let sel = scroll
        .selection
        .as_ref()
        .ok_or("should still have selection")
        .unwrap();
    assert_eq!(sel.cursor(), 0);
}

#[test]
fn test_scroll_state_reduce_scroll_action_to_bottom_follows_tail() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        total_visual_rows: 5,
        selection: Some(TranscriptSelection::new(0)),
        ..Default::default()
    };

    // -- Exec
    let changed = scroll.reduce_scroll_action(ScrollAction::ToBottom { select: true });

    // -- Check
    assert!(changed);
    assert_eq!(scroll.cursor_visual_row, 4);
    assert!(scroll.following_tail);
    let sel = scroll
        .selection
        .as_ref()
        .ok_or("should still have selection")
        .unwrap();
    assert_eq!(sel.cursor(), 4);
}

#[test]
fn test_scroll_state_reduce_scroll_action_page_up_down_move_by_page() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        cursor_visual_row: 10,
        total_visual_rows: 30,
        scroll_offset: 5,
        following_tail: false,
        entry_indices: (0..30).collect(),
        ..Default::default()
    };

    // -- Exec & Check
    let changed = scroll.reduce_scroll_action(ScrollAction::PageDown { select: false });
    assert!(changed);
    assert_eq!(scroll.cursor_visual_row, 18);
    assert_eq!(scroll.scroll_offset, 13);

    let changed = scroll.reduce_scroll_action(ScrollAction::PageUp { select: false });
    assert!(changed);
    assert_eq!(scroll.cursor_visual_row, 10);
    assert_eq!(scroll.scroll_offset, 5);
}

#[test]
fn test_scroll_state_reduce_scroll_action_focus_pane_toggles() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState::default();
    assert_eq!(scroll.pane_focus, PaneFocus::Input);

    // -- Exec & Check
    let changed = scroll.reduce_scroll_action(ScrollAction::FocusPaneLeft);
    assert!(changed);
    assert_eq!(scroll.pane_focus, PaneFocus::Transcript);

    let changed = scroll.reduce_scroll_action(ScrollAction::FocusPaneRight);
    assert!(changed);
    assert_eq!(scroll.pane_focus, PaneFocus::Input);
}

#[test]
fn test_scroll_state_enter_visual_mode_requires_transcript_focus() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState::default();
    assert_eq!(scroll.pane_focus, PaneFocus::Input);
    let mut status = StatusState::default();

    // -- Exec
    let entered = scroll.enter_visual_mode(&mut status);

    // -- Check
    assert!(!entered);
    assert_eq!(
        status.message.status_line,
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS
    );
    assert!(scroll.selection.is_none());
}

#[test]
fn test_scroll_state_enter_visual_mode_starts_selection_at_cursor() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        cursor_visual_row: 7,
        ..Default::default()
    };
    let mut status = StatusState::default();

    // -- Exec
    let entered = scroll.enter_visual_mode(&mut status);

    // -- Check
    assert!(entered);
    assert_eq!(status.message.status_line, "-- VISUAL --");
    let sel = scroll
        .selection
        .as_ref()
        .ok_or("should have selection")
        .unwrap();
    assert_eq!(sel.anchor(), 7);
    assert_eq!(sel.cursor(), 7);
}

#[test]
fn test_scroll_state_yank_selection_extracts_selected_rendered_rows() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        rendered_line_text: vec![
            "line 0".to_string(),
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
        ],
        ..Default::default()
    };
    let mut sel = TranscriptSelection::new(1);
    sel.set_cursor(2);
    scroll.selection = Some(sel);

    // -- Exec
    let payload = scroll.yank_selection();

    // -- Check
    assert_eq!(payload, Some("line 1\nline 2".to_string()));
    assert!(scroll.selection.is_none(), "selection cleared after yank");
}

#[test]
fn test_scroll_state_yank_selection_empty_payload_returns_none() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        selection: Some(TranscriptSelection::new(0)),
        ..Default::default()
    };

    // -- Exec
    let payload = scroll.yank_selection();

    // -- Check
    assert_eq!(payload, None);
    assert!(scroll.selection.is_none());
}

#[test]
fn test_scroll_state_yank_selection_without_selection_returns_none() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState::default();

    // -- Exec
    let payload = scroll.yank_selection();

    // -- Check
    assert_eq!(payload, None);
    assert!(scroll.selection.is_none());
}

#[test]
fn test_scroll_state_yank_selection_respects_rendered_line_start_row() {
    // -- Setup & Fixtures
    let mut sel = TranscriptSelection::new(11);
    sel.set_cursor(12);
    let mut scroll = ScrollState {
        rendered_line_start_row: 10,
        rendered_line_text: vec![
            "row 10".to_string(),
            "row 11".to_string(),
            "row 12".to_string(),
        ],
        selection: Some(sel),
        ..Default::default()
    };

    // -- Exec
    let payload = scroll.yank_selection();

    // -- Check
    assert_eq!(payload, Some("row 11\nrow 12".to_string()));
}

#[test]
fn test_scroll_state_scroll_helpers_nudge_offset_and_clear_tail_follow() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        following_tail: true,
        scroll_offset: 5,
        ..Default::default()
    };

    // -- Exec & Check
    scroll.scroll_transcript_line_up();
    assert_eq!(scroll.scroll_offset, 4);
    assert!(!scroll.following_tail);

    scroll.scroll_transcript_line_down();
    assert_eq!(scroll.scroll_offset, 5);

    scroll.scroll_transcript_page_up(3);
    assert_eq!(scroll.scroll_offset, 2);

    scroll.scroll_transcript_page_down(3);
    assert_eq!(scroll.scroll_offset, 5);

    scroll.scroll_transcript_to_top();
    assert_eq!(scroll.scroll_offset, 0);
    assert!(!scroll.following_tail);

    scroll.scroll_transcript_to_bottom();
    assert!(scroll.following_tail);
}

#[test]
fn test_scroll_state_pane_focus_cycles_both_directions() {
    // -- Setup & Fixtures
    let mut scroll = ScrollState::default();
    assert_eq!(scroll.pane_focus, PaneFocus::Input);

    // -- Exec & Check
    scroll.focus_next_pane();
    assert_eq!(scroll.pane_focus, PaneFocus::Transcript);
    scroll.focus_prev_pane();
    assert_eq!(scroll.pane_focus, PaneFocus::Input);
}
