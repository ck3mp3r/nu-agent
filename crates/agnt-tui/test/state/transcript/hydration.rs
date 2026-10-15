use super::*;

// ---------------------------------------------------------------------------
// Hydration reconstructs Tool blocks (task 53012ecc)
// ---------------------------------------------------------------------------

/// Hydration reconstructs one Tool block per call with its terminal status, and
/// attaches a persisted result display as its own ToolDisplay block.
#[test]
fn hydration_reconstructs_tool_blocks_and_display() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut tool = crate::state::ToolState::default();
    let mut compaction = crate::state::CompactionState::default();

    // -- Exec: a single finished tool call.
    state.transcript.hydrate_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[k8s__list_pods] → {} · done").with_tool_details(
                Some(r#"{"namespace":"prod"}"#.to_string()),
                Some("[]".to_string()),
                Some(true),
            ),
        ],
        None,
        &mut status,
        &mut tool,
        &mut compaction,
    );

    // -- Check: one Tool block, not a start block plus separate finish entries.
    assert_eq!(state.transcript.len(), 1);
    let block = state
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    let BlockSource::Tool { call, .. } = &block.source else {
        return Err("expected Tool block".into());
    };
    assert!(call.summary.contains("namespace"));
    assert_eq!(block.status, Some(ItemStatus::Done));

    // -- Setup & Fixtures: a tool call plus a standalone persisted display.
    let mut with_display = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut tool = crate::state::ToolState::default();
    let mut compaction = crate::state::CompactionState::default();
    let snapshots = vec![
        UiMessageSnapshot::new("tool", "→ {\"path\":\"a.rs\"}")
            .with_tool_name("edit".to_string())
            .with_tool_details(Some(r#"{"path":"a.rs"}"#.to_string()), None, Some(true)),
        UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(ToolDisplay {
            title: Some("edit a.rs".to_string()),
            sections: vec![ToolDisplaySection {
                label: Some("changes".to_string()),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    ];

    // -- Exec
    with_display.transcript.hydrate_from_messages(
        snapshots,
        None,
        &mut status,
        &mut tool,
        &mut compaction,
    );

    // -- Check: the Tool block keeps its terminal status and no preview; the
    // display hydrates as its own ToolDisplay block with Fill::Code.
    assert_eq!(
        with_display.transcript.len(),
        2,
        "preview must add one block"
    );
    let tool_block = with_display
        .transcript
        .blocks()
        .first()
        .ok_or("should have tool block")?;
    assert!(matches!(
        tool_block.source,
        BlockSource::Tool { preview: None, .. }
    ));
    assert_eq!(tool_block.status, Some(ItemStatus::Done));
    assert_eq!(tool_block.fill, agnt_core::transcript::ir::Fill::None);

    let preview_block = with_display
        .transcript
        .blocks()
        .get(1)
        .ok_or("should have preview block")?;
    assert!(matches!(
        preview_block.source,
        BlockSource::ToolDisplay { .. }
    ));
    assert_eq!(preview_block.fill, agnt_core::transcript::ir::Fill::Code);
    assert!(preview_block.source.plain_text().contains("+++ b"));

    // -- Setup & Fixtures: one role-"user" snapshot with multi-line content.
    let mut user = AppState::default();
    let mut status = crate::state::StatusState::default();
    let mut tool = crate::state::ToolState::default();
    let mut compaction = crate::state::CompactionState::default();

    // -- Exec
    user.transcript.hydrate_from_messages(
        vec![UiMessageSnapshot::new("user", "line1\n\nline2")],
        None,
        &mut status,
        &mut tool,
        &mut compaction,
    );

    // -- Check: one User block whose rail is contiguous on every rendered row.
    assert_eq!(
        user.transcript.len(),
        1,
        "a multi-line user message must hydrate as one block"
    );
    let block = user
        .transcript
        .blocks()
        .first()
        .ok_or("should have user block")?;
    assert!(
        matches!(
            block.source,
            BlockSource::Markdown {
                role: MessageRole::User,
                ..
            }
        ),
        "expected one User markdown block, got {:?}",
        block.source
    );
    assert_eq!(block.lane, agnt_core::transcript::ir::Lane::Marker("▏"));
    assert_eq!(block.fill, agnt_core::transcript::ir::Fill::Full);
    assert_eq!(block.source.plain_text(), "line1\n\nline2");

    let ctx = FrameContext {
        width: 80,
        now_millis: 0,
        cursor: false,
        selected: false,
    };
    let lines = crate::tui_renderer::layout(block, &ctx, &TuiTheme::default());
    assert!(
        lines.len() >= 2,
        "fixture must render more than one row; got {}",
        lines.len()
    );
    for (idx, line) in lines.iter().enumerate() {
        let marker = line.spans.get(1).ok_or("marker span must exist")?;
        assert!(
            marker.content.contains('▏'),
            "row {idx} must carry the user rail; got {:?}",
            marker.content
        );
    }
    Ok(())
}
