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

/// A summary entry the store authored hydrates as a compaction snapshot
/// carrying the raw summary: no headline, no wrapper tags.
#[test]
fn hydrate_store_entries_render_an_authored_summary_as_compaction() -> Result<()> {
    // -- Setup & Fixtures
    let entries = vec![
        StoredEntry {
            message: Message::User {
                content: vec![UserContent::Text(Text::new(wrapped_summary("rolled up")))],
            },
            author: Some(summary_author()),
            turn_seq: None,
        },
        StoredEntry::from(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("Response after compaction")),
        ]))),
    ];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "compaction");
    assert_eq!(snapshots[0].content(), "rolled up");
    assert_eq!(snapshots[1].role(), "assistant");
    Ok(())
}

/// A summary entry whose author the store lost still hydrates as a compaction
/// snapshot: the wrapper shape alone identifies it.
#[test]
fn hydrate_store_entries_render_an_authorless_summary_as_compaction() -> Result<()> {
    // -- Setup & Fixtures
    let entries = vec![StoredEntry::from(Message::User {
        content: vec![UserContent::Text(Text::new(wrapped_summary("rolled up")))],
    })];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    assert_eq!(snapshots[0].content(), "rolled up");
    Ok(())
}

/// The tracked sections rig-memory appends after the closing tag belong to the
/// model-facing wrapper, not to the displayed summary.
#[test]
fn hydrate_store_entries_strip_the_trailing_tracked_sections() -> Result<()> {
    // -- Setup & Fixtures
    let text = format!(
        "{}\n\n<files_read>\n/src/lib.rs\n</files_read>",
        wrapped_summary("rolled up")
    );
    let entries = vec![StoredEntry {
        message: Message::User {
            content: vec![UserContent::Text(Text::new(text))],
        },
        author: Some(summary_author()),
        turn_seq: None,
    }];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    assert_eq!(snapshots[0].content(), "rolled up");
    Ok(())
}

/// A message that mentions the headline outside the wrapper's first line stays
/// the user's own text, verbatim.
#[test]
fn hydrate_store_entries_keep_a_mid_text_headline_as_user() -> Result<()> {
    // -- Setup & Fixtures
    let text = format!("Intro line\n{}", wrapped_summary("rolled up"));
    let entries = vec![StoredEntry::from(Message::User {
        content: vec![UserContent::Text(Text::new(text.clone()))],
    })];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), text);
    Ok(())
}

/// A conversation compacted more than once keeps each summary, in order, as its
/// own compaction snapshot.
#[test]
fn hydrate_store_entries_render_each_summary_as_its_own_compaction() -> Result<()> {
    // -- Setup & Fixtures
    let entries = vec![
        StoredEntry {
            message: Message::User {
                content: vec![UserContent::Text(Text::new(wrapped_summary(
                    "first roll-up",
                )))],
            },
            author: Some(summary_author()),
            turn_seq: None,
        },
        StoredEntry::from(Message::User {
            content: vec![UserContent::Text(Text::new("a prompt"))],
        }),
        StoredEntry {
            message: Message::User {
                content: vec![UserContent::Text(Text::new(wrapped_summary(
                    "second roll-up",
                )))],
            },
            author: Some(summary_author()),
            turn_seq: None,
        },
    ];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.role())
            .collect::<Vec<_>>(),
        vec!["compaction", "user", "compaction"]
    );
    assert_eq!(snapshots[0].content(), "first roll-up");
    assert_eq!(snapshots[2].content(), "second roll-up");
    Ok(())
}

// -- Test Support

/// The wrapper rig-memory writes around the summary a compaction splices in.
fn wrapped_summary(summary: &str) -> String {
    format!(
        "The conversation before this point was compacted into this summary:\n\n\
         <summary>\n{summary}\n</summary>"
    )
}
