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

// ---------------------------------------------------------------------------
// Height index — incremental maintenance (task 7c4704ca)
// ---------------------------------------------------------------------------

/// Every mutation of the block list keeps the index valid at the indexed width
/// and its total equal to a wholesale rebuild of the same blocks. The store
/// maintains a per-block content-row cache instead of invalidating the index on
/// every push, so a streamed delta no longer forces a full re-measure.
#[test]
fn height_index_incremental_matches_wholesale_rebuild() -> Result<()> {
    // -- Setup & Fixtures
    let width = 60;
    let mut state = AppState::default();
    push_user(&mut state, "first user turn");
    push_assistant(&mut state, "assistant reply");
    state.transcript.rebuild_height_index(width);

    // -- Exec & Check: a markdown push.
    push_assistant(&mut state, "streamed delta one");
    assert_incremental_matches(&state, width, "after a markdown push")?;

    // -- Exec & Check: a pre-projected ToolDisplay push (code fill, so the
    // block carries margin rows).
    state.transcript.push_block(tool_display_block());
    assert_incremental_matches(&state, width, "after a ToolDisplay push")?;

    // -- Exec & Check: an insert in the middle of the list.
    state.transcript.insert_block_at(1, tool_display_block());
    assert_incremental_matches(&state, width, "after a mid-list insert")?;

    // -- Exec & Check: a truncate drops the tail.
    state.transcript.truncate(2);
    assert_incremental_matches(&state, width, "after a truncate")?;

    // -- Exec & Check: a truncate to empty.
    state.transcript.truncate(0);
    assert_incremental_matches(&state, width, "after a truncate to empty")?;
    Ok(())
}

/// Cap eviction drains the oldest blocks; the index must stay valid and match a
/// wholesale rebuild of the surviving blocks.
#[test]
fn height_index_incremental_survives_cap_eviction() -> Result<()> {
    // -- Setup & Fixtures
    let width = 60;
    let mut state = AppState::default();
    push_user(&mut state, "seed");
    state.transcript.rebuild_height_index(width);

    // -- Exec: push past the cap so the oldest blocks are evicted.
    for i in 0..MAX_TRANSCRIPT_BLOCKS {
        push_assistant(&mut state, &format!("line {i}"));
    }

    // -- Check: the store holds exactly the cap, and the index still matches a
    // wholesale rebuild of the survivors.
    assert_eq!(state.transcript.blocks().len(), MAX_TRANSCRIPT_BLOCKS);
    assert_incremental_matches(&state, width, "after cap eviction")?;
    Ok(())
}

// -- Test Support

/// Assert the store's index is valid at `width`, its total equals a wholesale
/// rebuild of the same blocks, and every block's start row matches that
/// rebuild. The per-block comparison pins the splice POSITION: a wrong-position
/// splice keeps the index "valid" and the total equal while relocating rows.
fn assert_incremental_matches(state: &AppState, width: usize, context: &str) -> Result<()> {
    assert!(
        state.transcript.height_index_valid_for(width),
        "index must stay valid {context}"
    );
    let mut wholesale = state.transcript.clone();
    wholesale.rebuild_height_index(width);
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale.total_visual_rows(),
        "incremental total must equal a wholesale rebuild {context}"
    );
    for index in 0..state.transcript.blocks().len() {
        assert_eq!(
            state.transcript.start_row_of(index),
            wholesale.start_row_of(index),
            "incremental start row of block {index} must equal a wholesale rebuild {context}"
        );
    }
    Ok(())
}

/// A pre-projected ToolDisplay block carrying a diff section, so the block
/// takes the code fill and its margin rows.
fn tool_display_block() -> Block {
    let display = Display {
        title: Some("edit".to_string()),
        sections: vec![DisplaySection {
            label: Some("src/main.rs".to_string()),
            kind: ContentKind::Diff {
                language: "rust".to_string(),
                path: "src/main.rs".to_string(),
            },
            content: "@@ -1,2 +1,2 @@\n-old line\n+new line\n".to_string(),
            stats: None,
        }],
    };
    Block {
        source: BlockSource::ToolDisplay {
            lines: display.project_lines(),
        },
        lane: Lane::Blank,
        fill: Fill::from_preview(&Some(display)),
        status: None,
    }
}
