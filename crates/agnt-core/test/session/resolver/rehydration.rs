use super::*;

// --- Compaction marker with stats test (task spec) ---

/// A Vec<StoreEntry> containing pre-compaction messages, a Marker, and
/// post-compaction messages produces a transcript where the compaction entry
/// appears at the correct position, has role "compaction", and carries the
/// full summary body.
#[test]
fn hydrate_store_entries_marker_shows_summary_body() {
    // -- Case 1: the marker sits between the pre- and post-compaction messages
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text::new("pre-compaction message 1"))],
        }),
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("pre-compaction reply")),
        ]))),
        StoreEntry::Marker(CompactionMarker::new(
            "History summarized: user asked about Rust, assistant explained ownership.".to_string(),
            Utc::now(),
        )),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text::new("post-compaction question"))],
        }),
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("post-compaction answer")),
        ]))),
    ];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));

    assert_eq!(
        snapshots.len(),
        5,
        "expected 5 snapshots, got: {snapshots:?}"
    );
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "pre-compaction message 1");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "pre-compaction reply");
    assert_eq!(
        snapshots[2].role(),
        "compaction",
        "expected compaction role at index 2"
    );
    assert!(
        snapshots[2].content().contains("History summarized"),
        "expected summary body in compaction content, got: {}",
        snapshots[2].content()
    );
    assert_eq!(snapshots[3].role(), "user");
    assert_eq!(snapshots[3].content(), "post-compaction question");
    assert_eq!(snapshots[4].role(), "assistant");
    assert_eq!(snapshots[4].content(), "post-compaction answer");

    // -- Case 2: a long summary is shown in full (no truncation)
    let long_summary = "x".repeat(600);
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        long_summary.clone(),
        Utc::now(),
    ))];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        snapshots[0].content(),
        long_summary,
        "expected full summary in content"
    );

    // -- Case 3: the marker body contains the summary text
    let summary = "The user asked about weather and got a response about rain.";
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        summary.to_string(),
        Utc::now(),
    ))];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    assert!(
        snapshots[0].content().contains(summary),
        "expected summary body in content, got: {}",
        snapshots[0].content()
    );
}

// --- tool_success rehydration tests ---

/// A row without a persisted verdict flag rehydrates as tool_success ==
/// Some(true) — output text is never consulted, whatever it looks like.
#[test]
fn hydrate_unflagged_tool_results_rehydrate_as_success() {
    // -- Setup & Fixtures
    // (name, call id, result text)
    let cases: Vec<(&str, &str, &str)> = vec![
        (
            "toolset error text",
            "call_fail_1",
            "Toolset error: command exited with code 1",
        ),
        ("plain success text", "call_ok_1", "file contents here"),
        ("permission denied text", "call_perm_1", "Permission denied"),
        (
            "doom loop text",
            "call_doom_1",
            "Doom loop detected: 'nu' called 5 times with identical arguments",
        ),
        (
            "tool quote text",
            "call_legacy_quote_1",
            "Tool 'nonexistent' is not available.",
        ),
        (
            "enriched denial text",
            "call_legacy_denial_1",
            "Permission denied by rule 'global:*' (scope: global)",
        ),
        (
            "unmatched failure text",
            "call_legacy_unmatched_1",
            "read failed: No such file or directory (os error 2)",
        ),
    ];

    // -- Exec & Check
    for (name, call_id, text) in cases {
        let entries = tool_call_entries(call_id, text, false);
        let snapshots = super::resolver::hydrate_transcript_from_store_entries(
            &entries,
            std::path::Path::new("."),
        );
        assert_eq!(snapshots.len(), 1, "{name}: one tool snapshot expected");
        assert_eq!(snapshots[0].role(), "tool", "{name}");
        assert_eq!(
            snapshots[0].tool_success(),
            Some(true),
            "{name}: unflagged rows must rehydrate as success — no text sniffing"
        );
    }
}

/// A persisted verdict flag decides tool_success, regardless of the output
/// text.
#[test]
fn hydrate_flagged_tool_results_use_persisted_verdict() {
    // -- Case 1: nu_agent_success=true wins over failure-shaped text
    let entries = tool_call_entries(
        "call_flag_ok_1",
        "Tool 'nonexistent' is not available.",
        false,
    );
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1, "flag must not change snapshot count");
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        Some(true),
        "persisted nu_agent_success=true must win over the `Tool '` text sniff"
    );

    // -- Case 2: nu_agent_success=false rehydrates as failure
    let entries = tool_call_entries(
        "call_flag_fail_1",
        "read failed: No such file or directory (os error 2)",
        true,
    );
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        Some(false),
        "persisted nu_agent_success=false must rehydrate as failure"
    );
}
