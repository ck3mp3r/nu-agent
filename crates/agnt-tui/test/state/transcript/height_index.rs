use super::*;

// ---------------------------------------------------------------------------
// Height index — measure()-based (task 53012ecc)
// ---------------------------------------------------------------------------

/// The height index totals the measured rows plus the separator rows the
/// SpacerStateMachine decides, exposes a viewport-covering block range, and
/// matches the layout row count for wrapping prose.
#[test]
fn height_index_totals_windows_and_matches_layout() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    push_user(&mut state, "short line");
    push_assistant(&mut state, "reply");
    let width = 80;

    // -- Exec
    state.transcript.rebuild_height_index(width);

    // -- Check: total = measure sum + separator rows (0 before the first block,
    // 2 between the two prose blocks).
    let measured: usize = state
        .transcript
        .blocks()
        .iter()
        .map(|block| crate::tui_renderer::measure(block, width, &TuiTheme::default()))
        .sum();
    assert_eq!(
        state.transcript.total_visual_rows(),
        measured + 2,
        "total_rows must equal measure sum plus SM separator rows"
    );

    // -- Setup & Fixtures: five user blocks.
    let mut windowed = AppState::default();
    for i in 0..5 {
        push_user(&mut windowed, &format!("line {i}"));
    }

    // -- Exec
    windowed.transcript.rebuild_height_index(width);

    // -- Check: viewport rows 0..2 covers blocks 0 and 1.
    let (first, last) = windowed.transcript.visible_window(0, 2);
    assert_eq!(first, 0);
    assert_eq!(last, 2);

    // -- Check: viewport rows 2..4 starts inside block 1.
    let (first, last) = windowed.transcript.visible_window(2, 2);
    assert_eq!(first, 1, "offset 2 is inside block 1");
    assert_eq!(last, 2, "block 2 starts at row 4, past the window end");

    // -- Setup & Fixtures: a single long wrapping prose line.
    let mut wrapping = AppState::default();
    let long_line = vec!["abc"; 60].join(" ");
    push_user(&mut wrapping, &long_line);

    // -- Exec & Check: the index matches the layout row count plus the trailing
    // separator at every width.
    for width in [24usize, 40, 80, 120] {
        wrapping.transcript.rebuild_height_index(width);
        let expected = rendered_row_count(&long_line, width) + 1;
        assert_eq!(
            wrapping.transcript.total_visual_rows(),
            expected,
            "height index must match rendered rows plus the trailing separator at width {width}"
        );
    }
    Ok(())
}
