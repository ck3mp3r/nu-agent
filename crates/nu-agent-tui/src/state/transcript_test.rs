use crate::state::{
    AppState, CompactionLine, CompactionStatus, PromptStatus, QueuedPrompt, ToolCallLine,
    ToolCallStatus, TranscriptRole,
};

/// The cap constant that will be defined in production code.
/// Tests reference this to avoid magic numbers.
const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

// ---------------------------------------------------------------------------
// Cap enforcement
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_evicts_oldest_when_exceeded() {
    let mut state = AppState::new();

    // Push one more than the cap
    for i in 0..=MAX_TRANSCRIPT_ENTRIES {
        state.push_transcript_line(TranscriptRole::User, format!("entry {i}"));
    }

    // RED: This will fail because transcript_preview grows unbounded
    assert_eq!(state.transcript_preview.len(), MAX_TRANSCRIPT_ENTRIES);

    // The oldest entry should have been evicted
    let first = &state.transcript_preview[0];
    assert_ne!(first.text(), "entry 0", "oldest entry should be evicted");
}

#[test]
fn transcript_cap_no_eviction_below_cap() {
    let mut state = AppState::new();

    for i in 0..MAX_TRANSCRIPT_ENTRIES {
        state.push_transcript_line(TranscriptRole::User, format!("entry {i}"));
    }

    // RED: This will pass even without the cap (exactly 2000 entries)
    assert_eq!(state.transcript_preview.len(), MAX_TRANSCRIPT_ENTRIES);

    // All entries should be present
    assert_eq!(
        state.transcript_preview[0].text(),
        "entry 0",
        "first entry should still be present"
    );
    assert_eq!(
        state.transcript_preview[MAX_TRANSCRIPT_ENTRIES - 1].text(),
        format!("entry {}", MAX_TRANSCRIPT_ENTRIES - 1),
        "last entry should still be present"
    );
}

#[test]
fn transcript_cap_empty_transcript_no_panic() {
    let mut state = AppState::new();

    // RED: This will pass even without the cap (no-op on empty)
    state.enforce_transcript_cap();

    assert!(state.transcript_preview.is_empty());
}

// ---------------------------------------------------------------------------
// Cap enforcement with mixed roles
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_enforced_with_alternating_turn_roles() {
    let mut state = AppState::new();

    // Push alternating turn roles (User/Assistant are excluded spacer pairs,
    // so no separators/spacers are inserted)
    for i in 0..MAX_TRANSCRIPT_ENTRIES / 2 {
        state.push_transcript_line(TranscriptRole::User, format!("user {i}"));
        state.push_transcript_line(TranscriptRole::Assistant, format!("assistant {i}"));
    }

    // Push one more to exceed cap
    state.push_transcript_line(TranscriptRole::User, "overflow");

    // 2001 entries exceed the 2000 cap; oldest is evicted
    assert_eq!(state.transcript_preview.len(), MAX_TRANSCRIPT_ENTRIES);
}

// ---------------------------------------------------------------------------
// Index shifting — streaming_message_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_streaming_message_start_shifted() {
    let mut state = AppState::new();
    state.streaming_message_start = Some(2010);

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(2010))
    assert_eq!(state.streaming_message_start, Some(10));
}

#[test]
fn shift_indices_streaming_message_start_evicted() {
    let mut state = AppState::new();
    state.streaming_message_start = Some(5);

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(5))
    assert_eq!(state.streaming_message_start, None);
}

#[test]
fn shift_indices_streaming_message_start_none_stays_none() {
    let mut state = AppState::new();
    state.streaming_message_start = None;

    state.shift_indices_after_eviction(2000);

    // RED: This will pass even with the stub (None stays None)
    assert_eq!(state.streaming_message_start, None);
}

// ---------------------------------------------------------------------------
// Index shifting — compaction_streaming_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_compaction_streaming_start_shifted() {
    let mut state = AppState::new();
    state.compaction_streaming_start = Some(2010);

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(2010))
    assert_eq!(state.compaction_streaming_start, Some(10));
}

#[test]
fn shift_indices_compaction_streaming_start_evicted() {
    let mut state = AppState::new();
    state.compaction_streaming_start = Some(5);

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(5))
    assert_eq!(state.compaction_streaming_start, None);
}

// ---------------------------------------------------------------------------
// Index shifting — prompt_items
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_prompt_items_shifted() {
    let mut state = AppState::new();
    state.prompt_items.push(QueuedPrompt {
        id: 1,
        prompt_text: "hello".to_string(),
        transcript_line_index: 2010,
        status: PromptStatus::Done,
    });

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays 2010)
    assert_eq!(state.prompt_items[0].transcript_line_index, 10);
}

#[test]
fn shift_indices_prompt_items_evicted_removed() {
    let mut state = AppState::new();
    state.prompt_items.push(QueuedPrompt {
        id: 1,
        prompt_text: "hello".to_string(),
        transcript_line_index: 5,
        status: PromptStatus::Done,
    });

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (item still present)
    assert!(state.prompt_items.is_empty());
}

// ---------------------------------------------------------------------------
// Index shifting — tool_call_items
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_tool_call_items_shifted() {
    let mut state = AppState::new();
    state.tool_call_items.push(ToolCallLine {
        id: 1,
        transcript_line_index: 2010,
        status: ToolCallStatus::Done,
        key: "test_tool".to_string(),
    });

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays 2010)
    assert_eq!(state.tool_call_items[0].transcript_line_index, 10);
}

#[test]
fn shift_indices_tool_call_items_evicted_removed() {
    let mut state = AppState::new();
    state.tool_call_items.push(ToolCallLine {
        id: 1,
        transcript_line_index: 5,
        status: ToolCallStatus::Done,
        key: "test_tool".to_string(),
    });

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (item still present)
    assert!(state.tool_call_items.is_empty());
}

// ---------------------------------------------------------------------------
// Index shifting — compaction_items
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_compaction_items_shifted() {
    let mut state = AppState::new();
    state.compaction_items.push(CompactionLine {
        transcript_line_index: 2010,
        source: "test".to_string(),
        status: CompactionStatus::Done,
    });

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays 2010)
    assert_eq!(state.compaction_items[0].transcript_line_index, 10);
}

#[test]
fn shift_indices_compaction_items_evicted_removed() {
    let mut state = AppState::new();
    state.compaction_items.push(CompactionLine {
        transcript_line_index: 5,
        source: "test".to_string(),
        status: CompactionStatus::Done,
    });

    state.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (item still present)
    assert!(state.compaction_items.is_empty());
}

// ---------------------------------------------------------------------------
// Index shifting — zero eviction is no-op
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_zero_eviction_is_noop() {
    let mut state = AppState::new();
    state.streaming_message_start = Some(10);
    state.compaction_streaming_start = Some(20);
    state.prompt_items.push(QueuedPrompt {
        id: 1,
        prompt_text: "hello".to_string(),
        transcript_line_index: 30,
        status: PromptStatus::Done,
    });
    state.tool_call_items.push(ToolCallLine {
        id: 1,
        transcript_line_index: 40,
        status: ToolCallStatus::Done,
        key: "tool".to_string(),
    });
    state.compaction_items.push(CompactionLine {
        transcript_line_index: 50,
        source: "test".to_string(),
        status: CompactionStatus::Done,
    });

    state.shift_indices_after_eviction(0);

    // RED: Stub does nothing, so this will pass (nothing changes)
    assert_eq!(state.streaming_message_start, Some(10));
    assert_eq!(state.compaction_streaming_start, Some(20));
    assert_eq!(state.prompt_items[0].transcript_line_index, 30);
    assert_eq!(state.tool_call_items[0].transcript_line_index, 40);
    assert_eq!(state.compaction_items[0].transcript_line_index, 50);
}

// ---------------------------------------------------------------------------
// Integration: push triggers eviction and shifts indices
// ---------------------------------------------------------------------------

#[test]
fn push_transcript_item_triggers_eviction_and_shifts_indices() {
    let mut state = AppState::new();

    // Add a prompt at index 2010
    state.prompt_items.push(QueuedPrompt {
        id: 1,
        prompt_text: "hello".to_string(),
        transcript_line_index: 2010,
        status: PromptStatus::Done,
    });

    // Push enough entries to trigger eviction
    for i in 0..=MAX_TRANSCRIPT_ENTRIES {
        state.push_transcript_line(TranscriptRole::User, format!("entry {i}"));
    }

    assert_eq!(state.transcript_preview.len(), MAX_TRANSCRIPT_ENTRIES);

    // 2001 User entries → eviction drains 1 entry (no separators since all same role)
    // Prompt at 2010 shifts to 2010 - 1 = 2009
    assert_eq!(state.prompt_items[0].transcript_line_index, 2009);
}

// ---------------------------------------------------------------------------
// Status lookup after eviction
// ---------------------------------------------------------------------------

#[test]
fn transcript_line_status_works_after_eviction() {
    let mut state = AppState::new();

    // Fill up to the cap
    for i in 0..MAX_TRANSCRIPT_ENTRIES {
        state.push_transcript_line(TranscriptRole::User, format!("entry {i}"));
    }

    // Push a prompt at the last index (1999)
    state.prompt_items.push(QueuedPrompt {
        id: 1,
        prompt_text: "hello".to_string(),
        transcript_line_index: MAX_TRANSCRIPT_ENTRIES - 1,
        status: PromptStatus::Done,
    });

    // Push one more to trigger eviction (drains 1 entry)
    state.push_transcript_line(TranscriptRole::User, "overflow");

    // Prompt at 1999 shifted to 1998 after eviction
    let status = state.transcript_line_status_for_index(MAX_TRANSCRIPT_ENTRIES - 2);
    assert!(
        status.is_some(),
        "status should still be findable after eviction"
    );
}
