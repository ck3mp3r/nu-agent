//! Unit tests for [`ScrollState`]: viewport sync, scroll actions, selection
//! hand-off, and pane focus.

use super::*;
use crate::interaction::reducer::VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS;
use crate::state::selection::TranscriptSelection;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_scroll_state_defaults_and_sync_after_render() -> Result<()> {
    // -- Setup & Fixtures
    let scroll = ScrollState::default();

    // -- Exec & Check: defaults follow tail with input focus.
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
    // The height index is owned by the transcript domain: a fresh store has
    // no index width yet (the renderer rebuilds it at the first frame).
    assert_eq!(
        crate::state::TranscriptStore::default().height_index_width,
        None
    );
    assert_eq!(scroll.pane_focus, PaneFocus::Input);

    // -- Exec & Check: sync pins the cursor while tailing.
    let mut tailing = ScrollState {
        viewport_height: 10,
        ..Default::default()
    };
    let max_scroll = tailing.sync_after_render(25);
    assert_eq!(max_scroll, 15);
    assert_eq!(tailing.total_visual_rows, 25);
    assert_eq!(tailing.max_scroll, 15);
    assert_eq!(tailing.cursor_visual_row, 24, "cursor rides the last row");

    // -- Exec & Check: sync leaves the cursor when not tailing.
    let mut detached = ScrollState {
        viewport_height: 10,
        following_tail: false,
        cursor_visual_row: 4,
        ..Default::default()
    };
    let max_scroll = detached.sync_after_render(25);
    assert_eq!(max_scroll, 15);
    assert_eq!(detached.cursor_visual_row, 4, "user-controlled cursor kept");

    // -- Exec & Check: sync clamps at zero when the viewport exceeds the rows.
    let mut clamped = ScrollState {
        viewport_height: 10,
        following_tail: false,
        ..Default::default()
    };
    let max_scroll = clamped.sync_after_render(3);
    assert_eq!(max_scroll, 0, "saturating_sub clamps at zero");
    assert_eq!(clamped.max_scroll, 0);
    assert_eq!(clamped.cursor_visual_row, 0);
    Ok(())
}

#[test]
fn test_scroll_state_sync_after_render_selection_sync() -> Result<()> {
    // -- Setup & Fixtures: a tail-following selection behind the pinned row.
    let mut tailing = ScrollState {
        viewport_height: 10,
        following_tail: true,
        selection: Some(TranscriptSelection::new(5)),
        ..Default::default()
    };

    // -- Exec
    let max_scroll = tailing.sync_after_render(25);

    // -- Check: the selection cursor rides the pinned row, the anchor stays.
    assert_eq!(max_scroll, 15);
    assert_eq!(tailing.cursor_visual_row, 24);
    let sel = tailing
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), 24, "selection cursor follows the pin");
    assert_eq!(sel.anchor(), 5, "anchor stays at the selection start");

    // -- Setup & Fixtures: no selection.
    let mut none = ScrollState {
        viewport_height: 10,
        following_tail: true,
        ..Default::default()
    };

    // -- Exec
    let max_scroll = none.sync_after_render(25);

    // -- Check: the cursor still pins and the max scroll is clamped.
    assert_eq!(max_scroll, 15);
    assert_eq!(none.cursor_visual_row, 24);
    assert!(none.selection.is_none());

    // -- Setup & Fixtures: a detached viewport with a selection.
    let mut detached = ScrollState {
        viewport_height: 10,
        following_tail: false,
        cursor_visual_row: 4,
        selection: Some(TranscriptSelection::new(4)),
        ..Default::default()
    };

    // -- Exec
    let max_scroll = detached.sync_after_render(25);

    // -- Check: the selection is untouched.
    assert_eq!(max_scroll, 15);
    assert_eq!(detached.cursor_visual_row, 4);
    let sel = detached
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), 4, "detached selection cursor kept");
    assert_eq!(sel.anchor(), 4);
    Ok(())
}

#[test]
fn test_scroll_state_sync_after_render_wheel_no_op_keeps_selection() -> Result<()> {
    // -- Setup & Fixtures: a tail-following selection synced to the pinned row.
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        following_tail: true,
        scroll_offset: 15,
        selection: Some(TranscriptSelection::new(5)),
        ..Default::default()
    };
    scroll.sync_after_render(25);

    // -- Exec: a wheel-down at the bottom with selection extension.
    let changed = scroll.reduce_scroll_action(ScrollAction::WheelDown { select: true });

    // -- Check: no visible change, and the selection stays on the cursor row.
    assert!(!changed, "wheel down at the bottom is a no-op");
    assert_eq!(scroll.cursor_visual_row, 24);
    let sel = scroll
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), scroll.cursor_visual_row);
    assert_eq!(sel.anchor(), 5);
    Ok(())
}

#[test]
fn test_scroll_state_reduce_scroll_action_line_moves() -> Result<()> {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        cursor_visual_row: 5,
        entry_indices: (0..20).collect(),
        total_visual_rows: 20,
        ..Default::default()
    };

    // -- Exec & Check: line down moves the cursor without scrolling.
    let changed = scroll.reduce_scroll_action(ScrollAction::LineDown { select: false });
    assert!(changed);
    assert_eq!(scroll.cursor_visual_row, 6);
    assert_eq!(scroll.scroll_offset, 0, "viewport did not scroll");

    // -- Exec & Check: line up exits tail follow and clamps at the top.
    let mut up = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        max_scroll: 10,
        total_visual_rows: 20,
        ..Default::default()
    };
    assert!(up.following_tail);
    let changed = up.reduce_scroll_action(ScrollAction::LineUp { select: false });
    assert!(changed);
    assert!(!up.following_tail);
    assert_eq!(
        up.scroll_offset, 9,
        "offset snaps to max_scroll, then the margin check nudges it"
    );
    assert_eq!(up.cursor_visual_row, 0, "cursor clamps at top");

    // -- Exec & Check: line down with select extends the selection.
    let mut selecting = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        selection: Some(TranscriptSelection::new(0)),
        entry_indices: (0..5).collect(),
        total_visual_rows: 5,
        ..Default::default()
    };
    let changed = selecting.reduce_scroll_action(ScrollAction::LineDown { select: true });
    assert!(changed);
    let sel = selecting
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), 1);
    assert_eq!(sel.anchor(), 0);

    // -- Exec & Check: line down without select keeps the selection cursor.
    let mut keeping = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        selection: Some(TranscriptSelection::new(0)),
        entry_indices: (0..5).collect(),
        total_visual_rows: 5,
        ..Default::default()
    };
    let changed = keeping.reduce_scroll_action(ScrollAction::LineDown { select: false });
    assert!(changed);
    let sel = keeping
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), 0, "selection cursor untouched");
    Ok(())
}

#[test]
fn test_scroll_state_reduce_scroll_action_jumps_and_paging() -> Result<()> {
    // -- Setup & Fixtures
    let mut top = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        total_visual_rows: 20,
        selection: Some(TranscriptSelection::new(2)),
        ..Default::default()
    };

    // -- Exec & Check: to top resets offset and selection.
    let changed = top.reduce_scroll_action(ScrollAction::ToTop { select: true });
    assert!(changed);
    assert_eq!(top.cursor_visual_row, 0);
    assert_eq!(top.scroll_offset, 0);
    assert!(!top.following_tail);
    let sel = top
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), 0);

    // -- Exec & Check: to bottom follows the tail.
    let mut bottom = ScrollState {
        pane_focus: PaneFocus::Transcript,
        total_visual_rows: 5,
        selection: Some(TranscriptSelection::new(0)),
        ..Default::default()
    };
    let changed = bottom.reduce_scroll_action(ScrollAction::ToBottom { select: true });
    assert!(changed);
    assert_eq!(bottom.cursor_visual_row, 4);
    assert!(bottom.following_tail);
    let sel = bottom
        .selection
        .as_ref()
        .ok_or("should still have selection")?;
    assert_eq!(sel.cursor(), 4);

    // -- Exec & Check: page down and page up move by a page.
    let mut paging = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        cursor_visual_row: 10,
        total_visual_rows: 30,
        scroll_offset: 5,
        following_tail: false,
        entry_indices: (0..30).collect(),
        ..Default::default()
    };
    let changed = paging.reduce_scroll_action(ScrollAction::PageDown { select: false });
    assert!(changed);
    assert_eq!(paging.cursor_visual_row, 18);
    assert_eq!(paging.scroll_offset, 13);
    let changed = paging.reduce_scroll_action(ScrollAction::PageUp { select: false });
    assert!(changed);
    assert_eq!(paging.cursor_visual_row, 10);
    assert_eq!(paging.scroll_offset, 5);

    // -- Exec & Check: focus pane toggles both directions.
    let mut focus = ScrollState::default();
    assert_eq!(focus.pane_focus, PaneFocus::Input);
    let changed = focus.reduce_scroll_action(ScrollAction::FocusPaneLeft);
    assert!(changed);
    assert_eq!(focus.pane_focus, PaneFocus::Transcript);
    let changed = focus.reduce_scroll_action(ScrollAction::FocusPaneRight);
    assert!(changed);
    assert_eq!(focus.pane_focus, PaneFocus::Input);
    Ok(())
}

#[test]
fn test_scroll_state_reduce_scroll_action_wheel_no_op() -> Result<()> {
    // -- Setup & Fixtures
    let mut at_top = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        max_scroll: 10,
        total_visual_rows: 20,
        following_tail: false,
        ..Default::default()
    };

    // -- Exec & Check: wheel up at the top changes nothing and reports no change.
    let changed = at_top.reduce_scroll_action(ScrollAction::WheelUp { select: false });
    assert!(!changed, "wheel up at the top is a no-op");
    assert_eq!(at_top.cursor_visual_row, 0);
    assert_eq!(at_top.scroll_offset, 0);
    assert!(!at_top.following_tail);

    // -- Exec & Check: wheel up mid-transcript moves the cursor by a wheel step.
    let mut mid_up = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        max_scroll: 20,
        total_visual_rows: 30,
        cursor_visual_row: 10,
        scroll_offset: 5,
        following_tail: false,
        ..Default::default()
    };
    let changed = mid_up.reduce_scroll_action(ScrollAction::WheelUp { select: false });
    assert!(changed, "wheel up mid-transcript moves the cursor");
    assert_eq!(mid_up.cursor_visual_row, 7, "cursor moved by WHEEL_LINES");
    assert_eq!(mid_up.scroll_offset, 4);

    // -- Exec & Check: wheel down at the bottom with tail follow is a no-op.
    let mut at_bottom = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        max_scroll: 10,
        total_visual_rows: 20,
        cursor_visual_row: 19,
        scroll_offset: 10,
        following_tail: true,
        ..Default::default()
    };
    let changed = at_bottom.reduce_scroll_action(ScrollAction::WheelDown { select: false });
    assert!(!changed, "wheel down at the bottom is a no-op");
    assert_eq!(at_bottom.cursor_visual_row, 19);
    assert_eq!(
        at_bottom.scroll_offset, 13,
        "raw offset overshoots max_scroll"
    );
    assert!(at_bottom.following_tail, "tail follow re-engaged");

    // -- Exec & Check: wheel down mid-transcript moves the cursor by a wheel step.
    let mut mid_down = ScrollState {
        pane_focus: PaneFocus::Transcript,
        viewport_height: 10,
        max_scroll: 20,
        total_visual_rows: 30,
        cursor_visual_row: 5,
        following_tail: false,
        ..Default::default()
    };
    let changed = mid_down.reduce_scroll_action(ScrollAction::WheelDown { select: false });
    assert!(changed, "wheel down mid-transcript moves the cursor");
    assert_eq!(mid_down.cursor_visual_row, 8, "cursor moved by WHEEL_LINES");
    assert_eq!(mid_down.scroll_offset, 2);
    Ok(())
}

#[test]
fn test_scroll_state_enter_visual_mode() -> Result<()> {
    // -- Setup & Fixtures
    let mut input_focus = ScrollState::default();
    assert_eq!(input_focus.pane_focus, PaneFocus::Input);
    let mut status = StatusState::default();

    // -- Exec & Check: visual mode requires transcript focus.
    let entered = input_focus.enter_visual_mode(&mut status);
    assert!(!entered);
    assert_eq!(
        status.message.status_line(),
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS
    );
    assert!(input_focus.selection.is_none());

    // -- Exec & Check: with transcript focus, the selection starts at the cursor.
    let mut transcript_focus = ScrollState {
        pane_focus: PaneFocus::Transcript,
        cursor_visual_row: 7,
        ..Default::default()
    };
    let mut status = StatusState::default();
    let entered = transcript_focus.enter_visual_mode(&mut status);
    assert!(entered);
    assert!(status.message.status_line().is_empty());
    let sel = transcript_focus
        .selection
        .as_ref()
        .ok_or("should have selection")?;
    assert_eq!(sel.anchor(), 7);
    assert_eq!(sel.cursor(), 7);
    Ok(())
}

#[test]
fn test_scroll_state_yank_selection() -> Result<()> {
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

    // -- Exec & Check: extracts the selected rendered rows and clears selection.
    let payload = scroll.yank_selection();
    assert_eq!(payload, Some("line 1\nline 2".to_string()));
    assert!(scroll.selection.is_none(), "selection cleared after yank");

    // -- Exec & Check: an empty payload returns none.
    let mut empty = ScrollState {
        selection: Some(TranscriptSelection::new(0)),
        ..Default::default()
    };
    let payload = empty.yank_selection();
    assert_eq!(payload, None);
    assert!(empty.selection.is_none());

    // -- Exec & Check: no selection returns none.
    let mut none = ScrollState::default();
    let payload = none.yank_selection();
    assert_eq!(payload, None);
    assert!(none.selection.is_none());

    // -- Exec & Check: yank respects the rendered line start row.
    let mut sel = TranscriptSelection::new(11);
    sel.set_cursor(12);
    let mut offset = ScrollState {
        rendered_line_start_row: 10,
        rendered_line_text: vec![
            "row 10".to_string(),
            "row 11".to_string(),
            "row 12".to_string(),
        ],
        selection: Some(sel),
        ..Default::default()
    };
    let payload = offset.yank_selection();
    assert_eq!(payload, Some("row 11\nrow 12".to_string()));
    Ok(())
}

#[test]
fn test_scroll_state_scroll_helpers_and_pane_focus() -> Result<()> {
    // -- Setup & Fixtures
    let mut scroll = ScrollState {
        following_tail: true,
        scroll_offset: 5,
        ..Default::default()
    };

    // -- Exec & Check: helpers nudge the offset and clear tail follow.
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

    // -- Exec & Check: pane focus cycles both directions.
    let mut focus = ScrollState::default();
    assert_eq!(focus.pane_focus, PaneFocus::Input);
    focus.focus_next_pane();
    assert_eq!(focus.pane_focus, PaneFocus::Transcript);
    focus.focus_prev_pane();
    assert_eq!(focus.pane_focus, PaneFocus::Input);
    Ok(())
}
