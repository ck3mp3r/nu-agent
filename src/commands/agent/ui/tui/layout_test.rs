use crate::commands::agent::ui::tui::layout::{
    LayoutInput,
    SIDE_PANE_COLLAPSE_COLUMNS,
    recompute_layout,
};

#[test]
fn narrow_size_class_collapses_side_pane_even_if_preferred_visible() {
    let layout = recompute_layout(LayoutInput {
        columns: 80,
        rows: 24,
        side_pane_visible: Some(true),
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
    });
    let at_threshold = recompute_layout(LayoutInput {
        columns: SIDE_PANE_COLLAPSE_COLUMNS,
        rows: 20,
        side_pane_visible: Some(true),
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
    });
    assert_eq!(one_row.status_event.height, 0);
    assert_eq!(one_row.input.height, 1);
    assert_eq!(one_row.transcript.height, 0);

    let two_rows = recompute_layout(LayoutInput {
        columns: 40,
        rows: 2,
        side_pane_visible: Some(true),
    });
    assert_eq!(two_rows.status_event.height, 1);
    assert_eq!(two_rows.input.height, 1);
    assert_eq!(two_rows.transcript.height, 0);
}
