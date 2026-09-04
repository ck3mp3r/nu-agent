use crate::state::{AppState, TranscriptRole};
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::TranscriptEntryKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The cap constant that will be defined in production code.
/// Tests reference this to avoid magic numbers.
const MAX_TRANSCRIPT_ENTRIES: usize = 2000;

// ---------------------------------------------------------------------------
// Cap enforcement
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_evicts_oldest_when_exceeded() {
    let mut state = AppState::default();

    // Push one more than the cap
    for i in 0..=MAX_TRANSCRIPT_ENTRIES {
        state
            .transcript
            .push_transcript_line(TranscriptRole::User, format!("entry {i}"));
    }

    // RED: This will fail because transcript_preview grows unbounded
    assert_eq!(state.transcript.entries.len(), MAX_TRANSCRIPT_ENTRIES);

    // The oldest entry should have been evicted
    let first = &state.transcript.entries[0];
    assert_ne!(first.text(), "entry 0", "oldest entry should be evicted");
}

#[test]
fn transcript_cap_no_eviction_below_cap() {
    let mut state = AppState::default();

    for i in 0..MAX_TRANSCRIPT_ENTRIES {
        state
            .transcript
            .push_transcript_line(TranscriptRole::User, format!("entry {i}"));
    }

    // RED: This will pass even without the cap (exactly 2000 entries)
    assert_eq!(state.transcript.entries.len(), MAX_TRANSCRIPT_ENTRIES);

    // All entries should be present
    assert_eq!(
        state.transcript.entries[0].text(),
        "entry 0",
        "first entry should still be present"
    );
    assert_eq!(
        state.transcript.entries[MAX_TRANSCRIPT_ENTRIES - 1].text(),
        format!("entry {}", MAX_TRANSCRIPT_ENTRIES - 1),
        "last entry should still be present"
    );
}

#[test]
fn transcript_cap_empty_transcript_no_panic() {
    let mut state = AppState::default();

    // RED: This will pass even without the cap (no-op on empty)
    state.transcript.enforce_transcript_cap();

    assert!(state.transcript.entries.is_empty());
}

// ---------------------------------------------------------------------------
// Cap enforcement with mixed roles
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_enforced_with_alternating_turn_roles() {
    let mut state = AppState::default();

    // Push alternating turn roles (User/Assistant are excluded spacer pairs,
    // so no separators/spacers are inserted)
    for i in 0..MAX_TRANSCRIPT_ENTRIES / 2 {
        state
            .transcript
            .push_transcript_line(TranscriptRole::User, format!("user {i}"));
        state
            .transcript
            .push_transcript_line(TranscriptRole::Assistant, format!("assistant {i}"));
    }

    // Push one more to exceed cap
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "overflow");

    // 2001 entries exceed the 2000 cap; oldest is evicted
    assert_eq!(state.transcript.entries.len(), MAX_TRANSCRIPT_ENTRIES);
}

// ---------------------------------------------------------------------------
// Index shifting — streaming_message_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_streaming_message_start_shifted() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(2010);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(2010))
    assert_eq!(state.transcript.assistant_stream_start, Some(10));
}

#[test]
fn shift_indices_streaming_message_start_evicted() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = Some(5);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(5))
    assert_eq!(state.transcript.assistant_stream_start, None);
}

#[test]
fn shift_indices_streaming_message_start_none_stays_none() {
    let mut state = AppState::default();
    state.transcript.assistant_stream_start = None;

    state.transcript.shift_indices_after_eviction(2000);

    // RED: This will pass even with the stub (None stays None)
    assert_eq!(state.transcript.assistant_stream_start, None);
}

// ---------------------------------------------------------------------------
// Index shifting — compaction_streaming_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_compaction_streaming_start_shifted() {
    let mut state = AppState::default();
    state.transcript.summary_stream_start = Some(2010);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(2010))
    assert_eq!(state.transcript.summary_stream_start, Some(10));
}

#[test]
fn shift_indices_compaction_streaming_start_evicted() {
    let mut state = AppState::default();
    state.transcript.summary_stream_start = Some(5);

    state.transcript.shift_indices_after_eviction(2000);

    // RED: Stub does nothing, so this will fail (stays Some(5))
    assert_eq!(state.transcript.summary_stream_start, None);
}

#[test]
fn no_turn_separator_between_user_and_assistant() {
    let mut state = AppState::default();

    // push_transcript_line pushes no reactive spacers — just the entries
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "prompt one");
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "response one");

    assert_eq!(state.transcript.entries.len(), 2);
    assert_eq!(state.transcript.entries[0].role(), Role::User);
    assert_eq!(state.transcript.entries[1].role(), Role::Assistant);
}

#[test]
fn no_turn_separator_for_same_role_sequences() {
    let mut state = AppState::default();

    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "line one");
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "line two");

    assert_eq!(
        state
            .transcript
            .entries
            .iter()
            .filter(|entry| entry.role() == Role::Separator)
            .count(),
        0
    );
}

#[test]
fn assistant_projection_cache_reuses_projected_markdown_for_same_input() {
    let mut state = AppState::default();
    let markdown = "```rust\nfn main() {\n    let x = 42;\n}\n```";

    let first = state.transcript.project_assistant_markdown_lines(markdown);
    let second = state.transcript.project_assistant_markdown_lines(markdown);

    assert_eq!(first, second);
}

#[test]
fn push_transcript_item_follows_tail_when_at_last_item() {
    let mut state = AppState::default();

    // Push first item — following_tail starts true, stays true
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "first");
    assert!(state.scroll.following_tail);

    // Push second item — should still follow
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "second");
    assert!(state.scroll.following_tail);

    // Push third item — should still follow
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "third");
    assert!(state.scroll.following_tail);
}

#[test]
fn push_transcript_item_stays_put_when_scrolled_up() {
    let mut state = AppState::default();

    // Push some items
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "first");
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "second");
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "third");

    // Scroll to top (user has scrolled up — disables following)
    state.scroll.scroll_transcript_to_top();
    assert!(!state.scroll.following_tail);
    assert_eq!(state.scroll.scroll_offset, 0);

    // Push new item — should NOT re-enable following, offset stays at 0
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "fourth");
    assert!(
        !state.scroll.following_tail,
        "following_tail should stay false when user has scrolled up"
    );
    assert_eq!(
        state.scroll.scroll_offset, 0,
        "scroll offset should stay at top when user has scrolled up"
    );
}

#[test]
fn push_transcript_item_follows_when_nothing_selected() {
    let mut state = AppState::default();

    // Initially following_tail is true (default)
    assert!(state.scroll.following_tail);

    // Push first item — following_tail stays true
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "first");
    assert!(
        state.scroll.following_tail,
        "first push should keep following_tail true"
    );
}

#[test]
fn clear_assistant_projection_cache_removes_all_entries() {
    let mut state = AppState::default();
    let markdown = "hello world";

    // Project once to populate the cache
    let first = state.transcript.project_assistant_markdown_lines(markdown);

    // Clearing the cache must not change the projected output
    state.transcript.clear_assistant_projection_cache();
    let second = state.transcript.project_assistant_markdown_lines(markdown);

    assert_eq!(first, second, "clearing cache must not change output");
}

#[test]
fn push_transcript_line_user_bold_markdown_emits_md_bold_span() -> Result<()> {
    let mut state = AppState::default();
    state
        .transcript
        .push_transcript_line(TranscriptRole::User, "hello **world**".to_string());
    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    let TranscriptEntryKind::User(m) = &last.kind else {
        panic!("expected User");
    };
    // Raw markdown is stored; verify it projects to MdBold at render time
    let bold = crate::markdown::render_markdown_lines(&m.markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold))
        .ok_or("should have MdBold span")?;
    assert_eq!(bold.text, "world");
    Ok(())
}

#[test]
fn push_transcript_line_assistant_bold_markdown_emits_md_bold_span() -> Result<()> {
    let mut state = AppState::default();
    state
        .transcript
        .push_transcript_line(TranscriptRole::Assistant, "hello **world**".to_string());
    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    let TranscriptEntryKind::Assistant(m) = &last.kind else {
        panic!("expected Assistant");
    };
    let bold = crate::markdown::render_markdown_lines(&m.markdown, None)
        .into_iter()
        .flat_map(|l| l.spans.into_iter())
        .find(|s| matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold))
        .ok_or("should have MdBold span")?;
    assert_eq!(bold.text, "world");
    Ok(())
}

#[test]
fn push_transcript_line_user_and_assistant_produce_identical_lines_for_same_text() -> Result<()> {
    let mut s1 = AppState::default();
    let mut s2 = AppState::default();
    let text = "**bold** and *italic* and `code`".to_string();
    s1.transcript
        .push_transcript_line(TranscriptRole::User, text.clone());
    s2.transcript
        .push_transcript_line(TranscriptRole::Assistant, text);
    let last1 = s1
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    let last2 = s2
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    let TranscriptEntryKind::User(u) = &last1.kind else {
        panic!();
    };
    let TranscriptEntryKind::Assistant(a) = &last2.kind else {
        panic!();
    };
    assert_eq!(
        u.markdown, a.markdown,
        "user and assistant prose must be byte-identical"
    );
    Ok(())
}

#[test]
fn push_transcript_line_user_fenced_code_block_produces_multiple_lines() -> Result<()> {
    let mut state = AppState::default();
    state.transcript.push_transcript_line(
        TranscriptRole::User,
        "```rust\nfn a() {}\nfn b() {}\n```".to_string(),
    );
    let last = state
        .transcript
        .entries
        .last()
        .ok_or("should have last transcript entry")?;
    let TranscriptEntryKind::User(m) = &last.kind else {
        panic!();
    };
    // Verify projection of the stored raw markdown yields multiple lines
    let projected = crate::markdown::render_markdown_lines(&m.markdown, None);
    assert!(projected.len() >= 2);
    Ok(())
}
