use super::*;

// ---------------------------------------------------------------------------
// Height index — measure()-based (task 53012ecc)
// ---------------------------------------------------------------------------

/// The height index totals the measured rows plus the separator rows the
/// planner's gap rules decide, exposes a viewport-covering block range, and
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
    let probe = crate::widgets::RatatuiBackend::new(80, 0);
    let measured: usize = state
        .transcript
        .entries()
        .iter()
        .map(|entry| agnt_render::RenderBackend::measure(&probe, entry, width))
        .sum();
    assert_eq!(
        state.transcript.total_visual_rows(),
        measured + 2,
        "total_rows must equal measure sum plus planner separator rows"
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

    // -- Exec & Check: a tool push whose diff display takes the code fill (so
    // the entry carries margin rows).
    state.transcript.push_entry(diff_tool_entry());
    assert_incremental_matches(&state, width, "after a tool push")?;

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

    // -- Exec: push past the cap so the oldest entries are evicted.
    for i in 0..MAX_TRANSCRIPT_ENTRIES {
        push_assistant(&mut state, &format!("line {i}"));
    }

    // -- Check: the store holds exactly the cap, and the index still matches a
    // wholesale rebuild of the survivors.
    assert_eq!(state.transcript.entries().len(), MAX_TRANSCRIPT_ENTRIES);
    assert_incremental_matches(&state, width, "after cap eviction")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Height index — geometry against the backend measure
// ---------------------------------------------------------------------------

/// Every entry's rows come from the backend the pane paints with, so the index
/// is a pure function of the entries and the width: the total is the measure
/// sum plus the planner's gap rows, and block 1 starts after block 0's
/// measured rows. The measure reads no theme data by construction — the
/// backend's `measure` takes no theme.
#[test]
fn height_index_rows_match_backend_measure() -> Result<()> {
    // -- Setup & Fixtures
    let width = 40;
    let long_prose = vec!["abc"; 60].join(" ");
    let mut prose = AppState::default();
    push_user(&mut prose, &long_prose);
    push_assistant(&mut prose, "reply");
    prose.transcript.rebuild_height_index(width);
    let probe = crate::widgets::RatatuiBackend::new(40, 0);
    let entries = prose.transcript.entries();
    let first = entries.first().ok_or("should have entry 0")?;

    // -- Exec & Check: the total is the measure sum plus the 2 separator rows
    // (user → assistant base 2, of which the assistant's kept top padding row
    // takes 1), and entry 1's cumulative start is the end of entry 0's measured
    // rows — the gap before entry 1 belongs to entry 1's own span.
    let measured: usize = entries
        .iter()
        .map(|entry| agnt_render::RenderBackend::measure(&probe, entry, width))
        .sum();
    assert_eq!(
        prose.transcript.total_visual_rows(),
        measured + 2,
        "total rows must equal the measure sum plus separator rows"
    );
    assert_eq!(
        prose.transcript.start_row_of(1),
        agnt_render::RenderBackend::measure(&probe, first, width),
        "entry 1 must start after entry 0's measured rows"
    );
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
    for index in 0..state.transcript.entries().len() {
        assert_eq!(
            state.transcript.start_row_of(index),
            wholesale.start_row_of(index),
            "incremental start row of block {index} must equal a wholesale rebuild {context}"
        );
    }
    Ok(())
}

/// A tool entry whose result is a diff display, so the entry takes the code
/// fill and its margin rows.
fn diff_tool_entry() -> TranscriptEntry {
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
    TranscriptEntry::Tool(ToolEntry {
        kind: ToolKind::Edit,
        call: ToolCall::new(
            ToolName("edit".to_string()),
            "",
            r#"{"path":"src/main.rs"}"#,
            CallLine {
                summary: "→ src/main.rs".to_string(),
            },
        ),
        preview: None,
        result: Some(display),
        status: ItemStatus::Done,
    })
}
