use super::*;

// --- hydrate_transcript_messages tests ---

/// Messages hydrate into snapshots in their original order.
#[test]
fn hydrate_store_entries_preserves_message_order() {
    // -- Setup & Fixtures
    let entries = vec![
        Message::User {
            content: vec![UserContent::Text(Text::new("First"))],
        },
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::Text(
            Text::new("Second"),
        )])),
        Message::User {
            content: vec![UserContent::Text(Text::new("Third"))],
        },
    ];

    // -- Exec
    let snapshots = hydrate(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "First");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "Second");
    assert_eq!(snapshots[2].role(), "user");
    assert_eq!(snapshots[2].content(), "Third");
}

/// A compaction summary the store happens to hold hydrates as an ordinary user
/// snapshot: summaries live in the rig-memory log, not in the session store.
#[test]
fn hydrate_store_entries_treats_a_summary_as_a_user_message() {
    // -- Setup & Fixtures
    let summary = format!(
        "{}\n\n<summary>\nrolled up\n</summary>",
        "The conversation before this point was compacted into this summary:"
    );
    let entries = vec![
        Message::User {
            content: vec![UserContent::Text(Text::new(summary))],
        },
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::Text(
            Text::new("Response after compaction"),
        )])),
    ];

    // -- Exec
    let snapshots = hydrate(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[1].role(), "assistant");
}
