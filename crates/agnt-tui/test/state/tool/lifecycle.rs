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

    // The push added rows, so the index is stale.
    assert!(!preview_state.transcript.height_index_valid_for(width));
    Ok(())
}
