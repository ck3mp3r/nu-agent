use super::*;

// ---------------------------------------------------------------------------
// Eviction shifts domain block_index bookkeeping (task 7f67b85b)
// ---------------------------------------------------------------------------

/// The shift method must move every surviving tool-call `block_index` down
/// by the evicted count, and drop indices that fall below the eviction.
#[test]
fn shift_bookkeeping_after_eviction_shifts_or_drops_tool_call_block_index() -> Result<()> {
    // -- Setup & Fixtures
    // (label, initial, evicted, expected)
    let cases: [(&str, Option<usize>, usize, Option<usize>); 3] = [
        ("shifted", Some(5), 3, Some(2)),
        ("dropped", Some(2), 3, None),
        ("zero count noop", Some(5), 0, Some(5)),
    ];

    // -- Exec & Check
    for (label, initial, evicted, expected) in cases {
        let mut state = AppState::default();
        state.tool.calls.push(ToolCallLine {
            id: 1,
            status: ToolCallStatus::InProgress,
            key: "edit\n{}".to_string(),
            block_index: initial,
        });

        state.shift_bookkeeping_after_eviction(evicted);

        let call = state.tool.calls.first().ok_or("should have call")?;
        assert_eq!(call.block_index, expected, "{label}");
    }
    Ok(())
}

#[test]
fn push_block_and_enforce_cap_report_evicted_counts() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check
    // Below the cap: no eviction.
    assert_eq!(state.transcript.push_block(simple_tool_block()), 0);
    assert_eq!(state.transcript.push_block(simple_tool_block()), 0);

    // Empty transcript: enforce is a no-op returning zero.
    let mut empty = AppState::default();
    assert_eq!(empty.transcript.enforce_transcript_cap(), 0);

    // Fill to the cap, then one more push evicts exactly the oldest block.
    for _ in 0..MAX_TRANSCRIPT_BLOCKS {
        state.transcript.push_block(simple_tool_block());
    }
    let evicted = state.transcript.push_block(simple_tool_block());
    assert_eq!(state.transcript.len(), MAX_TRANSCRIPT_BLOCKS);
    assert_eq!(evicted, 1);
    Ok(())
}

/// END-TO-END: eviction must keep `finish_tool_call` landing on the CORRECT
/// block. Start a tool call near the cap boundary, evict blocks in front of
/// it, then finish — the block AT THE SHIFTED INDEX must get the terminal
/// status, not some unrelated block.
#[test]
fn eviction_then_finish_tool_call_updates_block_at_shifted_index() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // Push to exactly the cap so the next tool-start evicts.
    for _ in 0..MAX_TRANSCRIPT_BLOCKS {
        state.transcript.push_block(simple_tool_block());
    }
    // Start a tool call: one block pushed, one evicted (the oldest); the tool
    // block lands at the final index (cap - 1).
    let mut start_evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "read",
        "{}",
        CallLine::from_json_summary("{}"),
        &mut start_evicted,
    );
    let tool_block_index = state
        .tool
        .calls
        .last()
        .ok_or("should have recorded call")?
        .block_index
        .ok_or("should have block index")?;
    assert_eq!(
        tool_block_index, MAX_TRANSCRIPT_BLOCKS,
        "fresh bookkeeping index is pre-compensated: store len 2000 minus 1, plus the 1 block evicted by this push"
    );
    // The dispatch seam shifts after every event in production; the test
    // replicates it because it drives the low-level methods. This shift
    // cancels the +1 pre-compensation above.
    state.shift_bookkeeping_after_eviction(start_evicted);
    assert_eq!(
        state
            .tool
            .calls
            .last()
            .ok_or("should have recorded call")?
            .block_index,
        Some(MAX_TRANSCRIPT_BLOCKS - 1),
        "after the seam shift the index is the tool block's true position"
    );

    // Push enough assistant blocks after it to evict the tool block itself.
    // Each push adds one block; each overflow evicts one.
    let assistant = block_from_item(&Message {
        role: MessageRole::Assistant,
        markdown: "filler".to_string(),
    });
    let mut total_evicted = 0usize;
    for _ in 0..3 {
        let push_evicted = state.transcript.push_block(assistant.clone());
        total_evicted += push_evicted;
        // The seam shift after each event, mirroring production.
        state.shift_bookkeeping_after_eviction(push_evicted);
    }

    // -- Exec
    state
        .tool
        .finish_tool_call(&mut state.transcript, "read", "{}", Some(true));

    // -- Check
    // The bookkeeping entry must have been shifted to the tool block's new
    // position; the finish must have landed on THAT block.
    let call = state.tool.calls.last().ok_or("should have recorded call")?;
    // The per-push seam shifts already tracked every eviction, so the
    // bookkeeping index equals the tool block's true position: the record
    // time position (cap - 1) minus the total evicted count.
    let expected_index = (MAX_TRANSCRIPT_BLOCKS - 1)
        .checked_sub(total_evicted)
        .filter(|_| (MAX_TRANSCRIPT_BLOCKS - 1) >= total_evicted);
    assert_eq!(
        call.block_index, expected_index,
        "bookkeeping index must be shifted by the evicted count"
    );
    if let Some(expected_index) = expected_index {
        let block = state
            .transcript
            .blocks()
            .get(expected_index)
            .ok_or("expected tool block must exist")?;
        assert!(
            matches!(block.source, BlockSource::Tool { .. }),
            "shifted index must still point at the tool block"
        );
        assert_eq!(
            block.status,
            Some(ItemStatus::Done),
            "finish must land on the shifted-index tool block"
        );
    }
    Ok(())
}

/// END-TO-END for the preview path: after eviction, `set_tool_preview` must
/// push the preview block directly after the Tool block at the SHIFTED index,
/// not at a stale one.
#[test]
fn eviction_then_set_tool_preview_pushes_preview_after_shifted_tool_block() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    for _ in 0..MAX_TRANSCRIPT_BLOCKS {
        state.transcript.push_block(simple_tool_block());
    }
    let mut start_evicted = 0usize;
    state.tool.start_tool_call(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        CallLine::from_json_summary(r#"{"path":"a.rs"}"#),
        &mut start_evicted,
    );
    let tool_block_index = state
        .tool
        .calls
        .last()
        .ok_or("should have recorded call")?
        .block_index
        .ok_or("should have block index")?;
    assert_eq!(
        tool_block_index, MAX_TRANSCRIPT_BLOCKS,
        "fresh bookkeeping index is pre-compensated: store len 2000 minus 1, plus the 1 block evicted by this push"
    );
    // The dispatch seam shifts after every event in production; the test
    // replicates it because it drives the low-level methods. This shift
    // cancels the +1 pre-compensation above.
    state.shift_bookkeeping_after_eviction(start_evicted);

    // Evict blocks in front of the tool block with assistant pushes (each
    // adds one block and evicts one overflow block). The tool block survives
    // and its index shifts down by the total evicted count.
    let filler = block_from_item(&Message {
        role: MessageRole::Assistant,
        markdown: "filler".to_string(),
    });
    let mut total_evicted = 0usize;
    for _ in 0..3 {
        let push_evicted = state.transcript.push_block(filler.clone());
        total_evicted += push_evicted;
        // The seam shift after each event, mirroring production.
        state.shift_bookkeeping_after_eviction(push_evicted);
    }
    assert!(total_evicted > 0, "eviction must have occurred");

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
    state.tool.set_tool_preview(
        &mut state.transcript,
        "edit",
        r#"{"path":"a.rs"}"#,
        preview,
        &mut preview_evicted,
    );
    state.shift_bookkeeping_after_eviction(preview_evicted);

    // -- Check
    // The per-push seam shifts already tracked every eviction, so the
    // bookkeeping index equals the tool block's true position: the record
    // time position (cap - 1) minus the total evicted count, minus the one
    // block the preview insert itself evicted (the store was already at cap).
    let shifted_index = (MAX_TRANSCRIPT_BLOCKS - 1) - total_evicted - preview_evicted;
    let block = state
        .transcript
        .blocks()
        .get(shifted_index)
        .ok_or("shifted-index tool block must exist")?;
    assert!(
        matches!(block.source, BlockSource::Tool { .. }),
        "the Tool block must stay at the shifted index"
    );
    let preview_block = state
        .transcript
        .blocks()
        .get(shifted_index + 1)
        .ok_or("preview block must follow the tool block")?;
    assert!(
        matches!(preview_block.source, BlockSource::ToolDisplay { .. }),
        "the preview block must land directly after the shifted-index tool block"
    );
    Ok(())
}
