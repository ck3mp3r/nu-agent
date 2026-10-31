use super::*;

use agnt_core::transcript::entry::TranscriptEntry;

/// Lane 1 reports only the model identity: no agent name, no pipe separator.
#[test]
fn lane_1_reports_model_only() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_active_agent_identity("coder");

    // -- Exec & Check: the agent name is not shown.
    let lane_1 = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let text: String = lane_1.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!text.contains("coder"));
    assert!(!text.contains("agent"));
    assert!(!text.contains('|'));

    // -- Exec & Check: the compact line reports lane 1 only.
    let status_line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let text: String = status_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!text.contains('|'));
    Ok(())
}

/// Hydration sets the latest total tokens from the metadata (or leaves it
/// none), and hydrates assistant/compaction markdown with styled spans.
#[test]
fn hydrate_transcript_tokens_and_markdown() -> Result<()> {
    // -- Setup & Fixtures
    let mut with_tokens = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    with_tokens.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), Some(14000));

    // -- Check: the latest total tokens are set.
    assert_eq!(
        with_tokens.state().status.tokens.latest_total_tokens,
        Some(14000)
    );

    // -- Setup & Fixtures
    let mut without = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    without.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    // -- Check: no value leaves the tokens none.
    assert_eq!(without.state().status.tokens.latest_total_tokens, None);

    // -- Setup & Fixtures: an assistant message with bold markdown.
    let mut bold = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    bold.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "hello **bold**")],
        None,
    );

    // -- Check: the bold span carries MdBold.
    let has_bold = bold
        .state()
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::Assistant => {
                Some(message.markdown.as_str())
            }
            _ => None,
        })
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .flat_map(|l| l.spans.into_iter())
        .any(|s| {
            s.text == "bold" && matches!(s.hint, agnt_core::transcript::ir::StyleHint::MdBold)
        });
    assert!(has_bold);

    // -- Setup & Fixtures: a compaction message with italic markdown.
    let mut italic = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    italic.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("compaction", "summary *italic*")],
        None,
    );

    // -- Check: the italic span carries MdItalic.
    let has_italic = italic
        .state()
        .transcript
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            TranscriptEntry::Message(message) if message.role == MessageRole::Assistant => {
                Some(message.markdown.as_str())
            }
            _ => None,
        })
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .flat_map(|l| l.spans.into_iter())
        .any(|s| {
            s.text == "italic" && matches!(s.hint, agnt_core::transcript::ir::StyleHint::MdItalic)
        });
    assert!(has_italic);
    Ok(())
}
