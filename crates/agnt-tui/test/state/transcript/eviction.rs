use super::*;

use agnt_core::session::tool_status::ToolStatus;

// ---------------------------------------------------------------------------
// Eviction shifts domain block_index bookkeeping (task 7f67b85b)
// ---------------------------------------------------------------------------

use crate::interaction::reducer::reduce;
use agnt_core::protocol::events::{SystemEvent, ToolCall, ToolCallId, ToolOutcome};

/// A `ToolStarted` event for `name`/`arguments`.
fn started(name: &str, arguments: &str) -> SystemEvent {
    SystemEvent::ToolStarted {
        call: ToolCall::new(
            ToolName(name.to_string()),
            "mcp",
            arguments,
            CallLine::from_json_summary(arguments),
        ),
    }
}

/// A successful `ToolCompleted` event for `name`/`arguments`.
fn completed(name: &str, arguments: &str) -> SystemEvent {
    SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName(name.to_string()),
            "closure",
            arguments,
            CallLine::from_json_summary(arguments),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "{}".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    }
}

/// Push one entry and shift the bookkeeping by the eviction it caused — the
/// same accounting `reduce` performs once per event.
fn push_and_shift(state: &mut AppState, entry: TranscriptEntry) -> usize {
    let before = state.transcript.evicted_total();
    state.transcript.push_entry(entry);
    let evicted = state.transcript.evicted_total() - before;
    state.shift_bookkeeping_after_eviction(evicted);
    evicted
}

/// Assistant prose entry used as cap filler.
fn assistant_filler(text: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::Assistant,
        markdown: text.to_string(),
    })
}

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
            key: ToolCallId::of("edit", "{}"),
            block_index: initial,
        });

        state.shift_bookkeeping_after_eviction(evicted);

        let call = state.tool.calls.first().ok_or("should have call")?;
        assert_eq!(call.block_index, expected, "{label}");
    }
    Ok(())
}

#[test]
fn push_entry_and_enforce_cap_report_evicted_counts() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec & Check
    // Below the cap: no eviction.
    assert_eq!(state.transcript.push_entry(simple_tool_entry()), 0);
    assert_eq!(state.transcript.push_entry(simple_tool_entry()), 0);

    // Empty transcript: enforce is a no-op returning zero.
    let mut empty = AppState::default();
    assert_eq!(empty.transcript.enforce_transcript_cap(), 0);

    // Fill to the cap, then one more push evicts exactly the oldest entry.
    for _ in 0..MAX_TRANSCRIPT_ENTRIES {
        state.transcript.push_entry(simple_tool_entry());
    }
    let evicted = state.transcript.push_entry(simple_tool_entry());
    assert_eq!(state.transcript.len(), MAX_TRANSCRIPT_ENTRIES);
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
    for _ in 0..MAX_TRANSCRIPT_ENTRIES {
        state.transcript.push_entry(simple_tool_entry());
    }
    // Start a tool call: one entry pushed, one evicted (the oldest); the tool
    // entry lands at the final index (cap - 1) and `reduce` shifts the fresh
    // bookkeeping entry to that true position in the same event.
    assert!(reduce(&mut state, started("read", "{}"), None));
    let tool_block_index = state
        .tool
        .calls
        .last()
        .ok_or("should have recorded call")?
        .block_index
        .ok_or("should have block index")?;
    assert_eq!(
        tool_block_index,
        MAX_TRANSCRIPT_ENTRIES - 1,
        "the event's bookkeeping shift leaves the index at the tool entry's true position"
    );

    // Push enough assistant entries after it to evict the tool entry itself.
    // Each push adds one entry; each overflow evicts one.
    let assistant = assistant_filler("filler");
    let mut total_evicted = 0usize;
    for _ in 0..3 {
        total_evicted += push_and_shift(&mut state, assistant.clone());
    }

    // -- Exec
    assert!(reduce(&mut state, completed("read", "{}"), None));

    // -- Check
    // The bookkeeping entry must have been shifted to the tool entry's new
    // position; the finish must have landed on THAT entry.
    let call = state.tool.calls.last().ok_or("should have recorded call")?;
    // The per-push seam shifts already tracked every eviction, so the
    // bookkeeping index equals the tool entry's true position: the record
    // time position (cap - 1) minus the total evicted count.
    let expected_index = (MAX_TRANSCRIPT_ENTRIES - 1)
        .checked_sub(total_evicted)
        .filter(|_| (MAX_TRANSCRIPT_ENTRIES - 1) >= total_evicted);
    assert_eq!(
        call.block_index, expected_index,
        "bookkeeping index must be shifted by the evicted count"
    );
    if let Some(expected_index) = expected_index {
        let entry = state
            .transcript
            .entries()
            .get(expected_index)
            .ok_or("expected tool entry must exist")?;
        let TranscriptEntry::Tool(tool) = entry else {
            return Err("shifted index must still point at the tool entry".into());
        };
        assert_eq!(
            tool.status,
            ItemStatus::Done,
            "finish must land on the shifted-index tool entry"
        );
    }
    Ok(())
}

/// END-TO-END for the preview path: after eviction, `set_tool_preview` must
/// land on the tool entry at the SHIFTED index and must not push a companion
/// entry.
#[test]
fn eviction_then_set_tool_preview_lands_on_shifted_tool_entry() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    for _ in 0..MAX_TRANSCRIPT_ENTRIES {
        state.transcript.push_entry(simple_tool_entry());
    }
    let edit_arguments = r#"{"path":"a.rs"}"#;
    assert!(reduce(&mut state, started("edit", edit_arguments), None));
    let tool_block_index = state
        .tool
        .calls
        .last()
        .ok_or("should have recorded call")?
        .block_index
        .ok_or("should have block index")?;
    assert_eq!(
        tool_block_index,
        MAX_TRANSCRIPT_ENTRIES - 1,
        "the event's bookkeeping shift leaves the index at the tool entry's true position"
    );

    // Evict entries in front of the tool entry with assistant pushes (each
    // adds one entry and evicts one overflow entry). The tool entry survives
    // and its index shifts down by the total evicted count.
    let filler = assistant_filler("filler");
    let mut total_evicted = 0usize;
    for _ in 0..3 {
        total_evicted += push_and_shift(&mut state, filler.clone());
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
    let len_before = state.transcript.len();
    assert!(state.tool.set_tool_preview(
        &mut state.transcript,
        &ToolCallId::of("edit", edit_arguments),
        &preview,
    ));

    // -- Check: the preview lands in place — no companion entry.
    assert_eq!(
        state.transcript.len(),
        len_before,
        "setting a preview pushes no entry"
    );
    let shifted_index = (MAX_TRANSCRIPT_ENTRIES - 1) - total_evicted;
    let entry = state
        .transcript
        .entries()
        .get(shifted_index)
        .ok_or("shifted-index tool entry must exist")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("the Tool entry must stay at the shifted index".into());
    };
    assert_eq!(
        tool.preview.as_ref(),
        Some(&preview),
        "the preview must land on the shifted-index tool entry"
    );
    Ok(())
}
