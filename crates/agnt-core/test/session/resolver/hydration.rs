use super::*;

// --- hydrate_transcript_from_store_entries tests ---

/// Messages and markers hydrate into snapshots in their original order.
#[test]
fn hydrate_store_entries_includes_messages_and_markers() {
    // -- Case 1: messages hydrate with their roles and content
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text::new("Hello"))],
        }),
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("Hi there")),
        ]))),
    ];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "Hello");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "Hi there");

    // -- Case 2: a marker hydrates as a compaction snapshot
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text::new("Hello"))],
        }),
        StoreEntry::Marker(CompactionMarker::new(
            "Summary of older messages".to_string(),
            Utc::now(),
        )),
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("Response after compaction")),
        ]))),
    ];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[1].role(), "compaction");
    assert_eq!(snapshots[2].role(), "assistant");

    // -- Case 3: order is preserved across messages and markers
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text::new("First"))],
        }),
        StoreEntry::Message(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("Second")),
        ]))),
        StoreEntry::Marker(CompactionMarker::new(
            "compaction summary".to_string(),
            Utc::now(),
        )),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text::new("Third"))],
        }),
    ];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 4);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "First");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "Second");
    assert_eq!(snapshots[2].role(), "compaction");
    assert_eq!(snapshots[3].role(), "user");
    assert_eq!(snapshots[3].content(), "Third");
}

/// A marker's content is its summary body, including the empty-summary case.
#[test]
fn hydrate_store_entries_marker_format() {
    // -- Case 1: the summary body is present in the content
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        "The user asked about weather and got a response.".to_string(),
        Utc::now(),
    ))];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    let content = snapshots[0].content();
    assert!(
        content.contains("The user asked about weather and got a response."),
        "Expected summary body in content, got: {content}"
    );

    // -- Case 2: an empty summary produces empty content
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        String::new(),
        Utc::now(),
    ))];
    let snapshots =
        super::resolver::hydrate_transcript_from_store_entries(&entries, std::path::Path::new("."));
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    let content = snapshots[0].content();
    assert!(
        content.is_empty(),
        "Expected empty content for empty-summary marker, got: {content}"
    );
}
