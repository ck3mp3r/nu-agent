use super::*;

// ---------------------------------------------------------------------------
// In-place tool block mutation (task 7ab65c6a)
// ---------------------------------------------------------------------------

/// `start_tool_call` creates exactly one Tool block; `finish_tool_call` mutates
/// that block's status in place without pushing a new one.
#[test]
fn start_and_finish_tool_call_mutate_one_block_in_place() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let mut evicted = 0usize;

    // -- Exec
    state.tool.start_tool_call(
        &mut state.transcript,
        "read",
        "{}",
        CallLine::from_json_summary("{}"),
        &mut evicted,
    );

    // -- Check: one Block only — no separate spacer, no separate display entry.
    assert_eq!(state.transcript.len(), 1);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));

    // -- Exec
    state
        .tool
        .finish_tool_call(&mut state.transcript, "read", "{}", Some(true), None);

    // -- Check: finish must not push a new block.
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));

    // -- Setup & Fixtures: a preview push with a valid height index.
    let mut preview_state = AppState::default();
    let width = 80usize;
    let mut evicted = 0usize;
    preview_state.tool.start_tool_call(
        &mut preview_state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    preview_state.transcript.rebuild_height_index(width);
    assert!(
        preview_state.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview push"
    );

    // -- Exec
    let preview = Display {
        title: Some("edit a.rs".to_string()),
        sections: vec![DisplaySection {
            label: Some("changes".to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    };
    let mut preview_evicted = 0usize;
    preview_state.tool.set_tool_preview(
        &mut preview_state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );

    // -- Check: the Tool block keeps no preview and no fill; the preview is a
    // separate ToolDisplay block pushed directly after it.
    assert_eq!(preview_state.transcript.len(), 2);
    let tool_block = preview_state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(matches!(
        tool_block.source,
        BlockSource::Tool { preview: None, .. }
    ));
    assert_eq!(tool_block.fill, Fill::None);

    let preview_block = preview_state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(matches!(
        preview_block.source,
        BlockSource::ToolDisplay { .. }
    ));
    assert_eq!(preview_block.fill, Fill::Code);
    assert!(tool_display_lines(preview_block).contains("+++ b"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Preview-path height index (task 69706655)
// ---------------------------------------------------------------------------

/// `set_tool_preview` inserts its ToolDisplay block through `insert_block_at`,
/// which maintains the height index incrementally. The index must therefore
/// stay valid — and equal to a wholesale rebuild — after the preview insert,
/// both when the preview lands at the tail and when it lands mid-list.
#[test]
fn set_tool_preview_keeps_height_index_valid() -> Result<()> {
    // -- Setup & Fixtures: a tool call with a valid index at width 80.
    let width = 80usize;
    let mut state = AppState::default();
    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    state.transcript.rebuild_height_index(width);
    assert!(
        state.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview insert"
    );

    // -- Exec: the preview lands at the tail (block 0, insert at 1).
    let mut preview_evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        edit_preview(),
        &mut preview_evicted,
    );

    // -- Check: the index is still valid and matches a wholesale rebuild.
    assert_eq!(state.transcript.len(), 2);
    assert!(
        state.transcript.height_index_valid_for(width),
        "a tail preview insert must leave the height index valid"
    );
    assert_eq!(
        state.transcript.total_visual_rows(),
        wholesale_total_rows(&state, width),
        "the incremental index must match a wholesale rebuild after a tail preview insert"
    );

    // -- Setup & Fixtures: a mid-list insert — a user block after the tool call.
    let mut mid = AppState::default();
    let mut evicted = 0usize;
    mid.tool.start_tool_call(
        &mut mid.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    mid.transcript.push_block(Block {
        source: BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "later turn".to_string(),
        },
        lane: Lane::Marker("\u{258f}"),
        fill: Fill::Full,
        status: None,
    });
    mid.transcript.rebuild_height_index(width);
    assert!(
        mid.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the mid-list preview insert"
    );

    // -- Exec: the preview lands at index 1, before the user block.
    let mut preview_evicted = 0usize;
    mid.tool.set_tool_preview(
        &mut mid.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        edit_preview(),
        &mut preview_evicted,
    );

    // -- Check: the mid-list insert keeps the index valid and equal to a
    // wholesale rebuild.
    assert_eq!(mid.transcript.len(), 3);
    assert!(
        mid.transcript.height_index_valid_for(width),
        "a mid-list preview insert must leave the height index valid"
    );
    assert_eq!(
        mid.transcript.total_visual_rows(),
        wholesale_total_rows(&mid, width),
        "the incremental index must match a wholesale rebuild after a mid-list preview insert"
    );
    Ok(())
}

// -- Test Support

/// The diff preview `set_tool_preview` attaches in these tests.
fn edit_preview() -> Display {
    Display {
        title: Some("edit a.rs".to_string()),
        sections: vec![DisplaySection {
            label: Some("changes".to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    }
}

/// Total visual rows a wholesale rebuild of the same blocks produces.
fn wholesale_total_rows(state: &AppState, width: usize) -> usize {
    let mut wholesale = state.transcript.clone();
    wholesale.rebuild_height_index(width);
    wholesale.total_visual_rows()
}
