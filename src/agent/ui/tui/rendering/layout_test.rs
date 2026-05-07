use crate::agent::ui::tui::rendering::layout::{
    LayoutInput,
    SIDE_PANE_COLLAPSE_COLUMNS,
    input_content_row_count,
    input_cursor_row_col,
    input_pane_height_for_content,
    recompute_layout,
    wrapped_input_rows,
};

#[test]
fn narrow_size_class_collapses_side_pane_even_if_preferred_visible() {
    let layout = recompute_layout(LayoutInput {
        columns: 80,
        rows: 24,
        side_pane_visible: Some(true),
        input_height: None,
    });

    assert!(layout.side_pane.is_none());
    assert_eq!(layout.transcript.width, 80);
}

#[test]
fn normal_size_class_uses_single_column_when_side_not_requested() {
    let layout = recompute_layout(LayoutInput {
        columns: 110,
        rows: 30,
        side_pane_visible: None,
        input_height: None,
    });

    assert!(layout.side_pane.is_none());
    assert_eq!(layout.transcript.width, 110);
    assert_eq!(layout.status_event.width, 110);
    assert_eq!(layout.input.width, 110);
}

#[test]
fn wide_size_class_shows_side_pane_when_requested() {
    let layout = recompute_layout(LayoutInput {
        columns: 160,
        rows: 40,
        side_pane_visible: Some(true),
        input_height: None,
    });

    let side = layout
        .side_pane
        .expect("side pane should be visible in wide class when requested");
    assert!(side.width > 0);
    assert_eq!(layout.transcript.width + side.width, 160);
    assert_eq!(side.height, 40);
}

#[test]
fn collapse_threshold_boundary_is_deterministic() {
    let just_below = recompute_layout(LayoutInput {
        columns: SIDE_PANE_COLLAPSE_COLUMNS - 1,
        rows: 20,
        side_pane_visible: Some(true),
        input_height: None,
    });
    let at_threshold = recompute_layout(LayoutInput {
        columns: SIDE_PANE_COLLAPSE_COLUMNS,
        rows: 20,
        side_pane_visible: Some(true),
        input_height: None,
    });

    assert!(just_below.side_pane.is_none());
    assert!(at_threshold.side_pane.is_some());
}

#[test]
fn geometry_is_always_non_negative_and_clipped_to_terminal() {
    for columns in [0, 1, 20, 79, 80, 119, 120, 240] {
        for rows in [0, 1, 2, 3, 10, 50] {
            let layout = recompute_layout(LayoutInput {
                columns,
                rows,
                side_pane_visible: Some(true),
                input_height: None,
            });

            let panes = [
                layout.transcript,
                layout.status_event,
                layout.input,
                layout.side_pane.unwrap_or_default(),
            ];

            for pane in panes {
                assert!(pane.x <= columns);
                assert!(pane.y <= rows);
                assert!(pane.width <= columns.saturating_sub(pane.x));
                assert!(pane.height <= rows.saturating_sub(pane.y));
            }

            assert_eq!(layout.transcript.width, layout.input.width);
            assert_eq!(layout.transcript.width, layout.status_event.width);
            assert_eq!(
                layout.transcript.height + layout.status_event.height + layout.input.height,
                rows
            );
        }
    }
}

#[test]
fn minimum_size_fallback_prioritizes_input_and_clips_status() {
    let one_row = recompute_layout(LayoutInput {
        columns: 40,
        rows: 1,
        side_pane_visible: Some(true),
        input_height: None,
    });
    assert_eq!(one_row.status_event.height, 0);
    assert_eq!(one_row.input.height, 1);
    assert_eq!(one_row.transcript.height, 0);

    let two_rows = recompute_layout(LayoutInput {
        columns: 40,
        rows: 2,
        side_pane_visible: Some(true),
        input_height: None,
    });
    assert_eq!(two_rows.status_event.height, 1);
    assert_eq!(two_rows.input.height, 1);
    assert_eq!(two_rows.transcript.height, 0);
}

#[test]
fn input_height_grows_with_newlines_and_wrap_and_is_clamped() {
    let h_short = input_pane_height_for_content("x", 80);
    assert_eq!(h_short, 2);

    let h_multiline = input_pane_height_for_content("a\nb\nc", 80);
    assert!(h_multiline > h_short);

    let h_wrapped = input_pane_height_for_content("abcdefghij", 4);
    assert!(h_wrapped > h_short);

    let h_clamped = input_pane_height_for_content(&"x".repeat(300), 4);
    assert_eq!(h_clamped, 8);
}

#[test]
fn wrapped_rows_and_cursor_mapping_handle_mixed_newline_and_wrap() {
    let rows = wrapped_input_rows("ab\n12345", 3);
    assert_eq!(rows, vec!["ab".to_string(), "123".to_string(), "45".to_string()]);
    assert_eq!(input_content_row_count("ab\n12345", 3), 3);

    assert_eq!(input_cursor_row_col("ab\n12345", 0, 3), (0, 0));
    assert_eq!(input_cursor_row_col("ab\n12345", 2, 3), (0, 2));
    assert_eq!(input_cursor_row_col("ab\n12345", 3, 3), (1, 0));
    assert_eq!(input_cursor_row_col("ab\n12345", 6, 3), (2, 0));
    assert_eq!(input_cursor_row_col("ab\n12345", 8, 3), (2, 2));
}

#[test]
fn layout_honors_requested_input_height_while_keeping_status_visible() {
    let layout = recompute_layout(LayoutInput {
        columns: 80,
        rows: 10,
        side_pane_visible: Some(false),
        input_height: Some(6),
    });

    assert_eq!(layout.input.height, 6);
    assert_eq!(layout.status_event.height, 1);
    assert_eq!(layout.transcript.height + layout.status_event.height + layout.input.height, 10);
}
