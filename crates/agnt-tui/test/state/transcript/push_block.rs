use super::*;

// ---------------------------------------------------------------------------
// Store holds content blocks only — separators are a render-time concern
// (task 4609782e). The store never inserts Spacer blocks; the number of
// visual separator rows between two blocks is decided by the
// SpacerStateMachine during rendering (see spacer_test.rs).
// ---------------------------------------------------------------------------

/// Pushing blocks stores one content block each, in order, with no Spacer
/// blocks, whatever the mix of user, assistant, tool, notice, and display
/// blocks.
#[test]
fn push_block_stores_content_blocks_without_spacers() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check: no spacer before the first block.
    push_user(&mut state, "prompt one");
    assert_eq!(state.transcript.len(), 1);
    assert!(!matches!(
        state.transcript.blocks()[0].source,
        BlockSource::Spacer
    ));

    // -- Exec & Check: two user blocks.
    let mut two_user = AppState::default();
    push_user(&mut two_user, "prompt one");
    push_user(&mut two_user, "prompt two");
    assert_eq!(two_user.transcript.len(), 2);
    assert!(!matches!(
        two_user.transcript.blocks()[0].source,
        BlockSource::Spacer
    ));
    assert!(matches!(
        two_user.transcript.blocks()[1].source,
        BlockSource::Markdown {
            role: MessageRole::User,
            ..
        }
    ));

    // -- Exec & Check: user then assistant.
    let mut user_assistant = AppState::default();
    push_user(&mut user_assistant, "prompt one");
    push_assistant(&mut user_assistant, "response one");
    assert_eq!(user_assistant.transcript.len(), 2);
    assert!(matches!(
        user_assistant.transcript.blocks()[1].source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));

    // -- Exec & Check: assistant then user.
    let mut assistant_user = AppState::default();
    push_assistant(&mut assistant_user, "response one");
    push_user(&mut assistant_user, "prompt two");
    assert_eq!(assistant_user.transcript.len(), 2);
    assert!(matches!(
        assistant_user.transcript.blocks()[0].source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));

    // -- Exec & Check: tool then assistant.
    let mut tool_assistant = AppState::default();
    tool_assistant.transcript.push_block(simple_tool_block());
    push_assistant(&mut tool_assistant, "response one");
    assert_eq!(tool_assistant.transcript.len(), 2);
    assert!(matches!(
        tool_assistant.transcript.blocks()[1].source,
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            ..
        }
    ));

    // -- Exec & Check: three alternating blocks.
    let mut three = AppState::default();
    push_user(&mut three, "prompt one");
    push_assistant(&mut three, "response one");
    push_user(&mut three, "prompt two");
    assert_eq!(three.transcript.len(), 3);
    for block in three.transcript.blocks() {
        assert!(matches!(block.source, BlockSource::Markdown { .. }));
    }

    // -- Exec & Check: notice and tool display store content only.
    let mut mixed = AppState::default();
    let notice = agnt_core::transcript::items::Notice {
        kind: NoticeKind::Compaction,
        text: "Compaction".to_string(),
    };
    mixed.transcript.push_block(block_from_item(&notice));
    mixed.transcript.push_block(Block {
        source: BlockSource::ToolDisplay { lines: Vec::new() },
        lane: agnt_core::transcript::ir::Lane::Blank,
        fill: agnt_core::transcript::ir::Fill::None,
        status: None,
    });
    assert_eq!(mixed.transcript.len(), 2);
    assert!(matches!(
        mixed.transcript.blocks()[1].source,
        BlockSource::ToolDisplay { .. }
    ));

    // -- Exec & Check: a pushed block keeps the item's lane and fill.
    let mut lane = AppState::default();
    let msg = Message {
        role: MessageRole::User,
        markdown: "hello".to_string(),
    };
    lane.transcript.push_block(block_from_item(&msg));
    let block = lane
        .transcript
        .blocks()
        .first()
        .ok_or("should have block")?;
    assert_eq!(block.lane, agnt_core::transcript::ir::Lane::Marker("▏"));
    assert_eq!(block.fill, agnt_core::transcript::ir::Fill::Full);

    // -- Exec & Check: a tool block carries its status.
    let mut tool_state = AppState::default();
    let tool = agnt_core::transcript::items::Tool {
        name: ToolName("read".to_string()),
        call: CallLine::from_json_summary("{}"),
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    };
    tool_state.transcript.push_block(Block {
        source: tool.source(),
        lane: tool.lane(),
        fill: tool.fill(),
        status: Some(ItemStatus::InProgress),
    });
    let block = tool_state
        .transcript
        .blocks()
        .first()
        .ok_or("should have block")?;
    assert!(matches!(block.source, BlockSource::Tool { .. }));
    assert_eq!(block.status, Some(ItemStatus::InProgress));
    Ok(())
}
