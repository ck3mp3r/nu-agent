use super::*;

// ---------------------------------------------------------------------------
// In-place tool block mutation (task 7ab65c6a)
// ---------------------------------------------------------------------------

#[test]
fn start_tool_call_creates_exactly_one_block() {
    let mut state = AppState::default();

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "k8s__list_pods",
        r#"{"namespace":"prod"}"#,
        CallLine::from_json_summary(r#"{"namespace":"prod"}"#),
        &mut evicted,
    );

    // One Block only — no separate spacer, no separate display entry.
    assert_eq!(state.transcript.len(), 1);
    assert!(matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Tool { .. }
    ));
}

#[test]
fn set_tool_preview_pushes_adjacent_tool_display_block() -> Result<()> {
    let mut state = AppState::default();

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "changes".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    };
    let mut preview_evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );

    // -- Check: the Tool block keeps no preview and no fill; the preview is a
    // separate ToolDisplay block pushed directly after it.
    assert_eq!(
        state.transcript.len(),
        2,
        "preview must push its own block after the Tool block"
    );
    let tool_block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(
        matches!(tool_block.source, BlockSource::Tool { preview: None, .. }),
        "the Tool block must keep preview None"
    );
    assert_eq!(
        tool_block.fill,
        Fill::None,
        "the Tool block must keep Fill::None so the call line stays untinted"
    );

    let preview_block = state
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview must be a ToolDisplay block"
    );
    assert_eq!(
        preview_block.fill,
        Fill::Code,
        "a diff preview block must carry Fill::Code"
    );
    assert!(
        tool_display_lines(preview_block).contains("+++ b"),
        "the preview block must carry the projected diff content"
    );
    Ok(())
}

#[test]
fn set_tool_preview_invalidates_height_index() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let width = 80usize;

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut evicted,
    );
    // Build the height index so it is valid, then push the preview block.
    state.transcript.rebuild_height_index(width);
    assert!(
        state.transcript.height_index_valid_for(width),
        "precondition: index must be valid before the preview push"
    );

    // -- Exec
    let preview = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "changes".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a\n+++ b\n".to_string(),
            stats: None,
        }],
    };
    let mut preview_evicted = 0usize;
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );

    // -- Check: the push added rows, so the index is stale.
    assert!(
        !state.transcript.height_index_valid_for(width),
        "set_tool_preview must invalidate the height index"
    );
    Ok(())
}

#[test]
fn finish_tool_call_mutates_status_on_existing_block() {
    let mut state = AppState::default();

    let mut evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "read",
        "{}",
        CallLine::from_json_summary("{}"),
        &mut evicted,
    );
    state
        .tool
        .finish_tool_call(&mut state.transcript, "read", "{}", Some(true));

    assert_eq!(
        state.transcript.len(),
        1,
        "finish must not push a new block"
    );
    assert_eq!(state.transcript.blocks()[0].status, Some(ItemStatus::Done));
}
