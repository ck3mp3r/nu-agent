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

/// WHEN the store carries the frame rig-memory 0.47 writes, THE hydration SHALL
/// carry the summary alone into the compaction snapshot: no opening headline, no
/// `<summary>` lines.
#[test]
fn hydrate_store_entries_strip_the_rig_0_47_frame() -> Result<()> {
    // -- Setup & Fixtures
    let summary = "## Goal\nShip the resolver fix";
    let entries = vec![StoredEntry {
        message: Message::User {
            content: vec![UserContent::Text(Text::new(rig_frame(summary)))],
        },
        author: Some(summary_author()),
        turn_seq: None,
    }];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    assert_eq!(snapshots[0].content(), summary);
    Ok(())
}

/// WHEN the frame carries file lists and no summary text, THE hydration SHALL
/// carry the stored frame whole, so the row never renders empty.
#[test]
fn hydrate_store_entries_keep_the_frame_when_the_summary_is_empty() -> Result<()> {
    // -- Setup & Fixtures
    let text = rig_frame("\n\n<read-files>\n/src/lib.rs\n</read-files>");
    let entries = vec![StoredEntry {
        message: Message::User {
            content: vec![UserContent::Text(Text::new(text.clone()))],
        },
        author: Some(summary_author()),
        turn_seq: None,
    }];

    // -- Exec
    let snapshots = hydrate_entries(&entries, std::path::Path::new("."));

    // -- Check
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    assert_eq!(snapshots[0].content(), text);
    Ok(())
}

/// WHEN the store's entry carries a tool result, THE hydration SHALL carry its
/// text into the snapshot, and the projection SHALL hand it to the surface as
/// the completion's payload and text — the data the drawer derives the body
/// from, exactly as the live path does.
#[test]
fn hydrate_store_entries_carry_the_tool_result_text() -> Result<()> {
    // -- Setup & Fixtures: one answered call whose result text the store kept.
    let entries = tool_call_entries("call-1", "the file's text", false)?;

    // -- Exec
    let snapshots = hydrate(&entries, std::path::Path::new("."));
    let (records, _total) = crate::protocol::contracts::project_snapshots(snapshots);

    // -- Check: one replayed call, and its result rides the completion.
    let outcomes: Vec<&crate::protocol::events::ToolOutcome> = records
        .iter()
        .filter_map(|record| match record {
            crate::protocol::events::SessionRecord::Tool(
                crate::protocol::events::ToolRecord::Completed { outcome, .. },
            ) => Some(outcome),
            _ => None,
        })
        .collect();
    assert_eq!(outcomes.len(), 1, "one replayed call, one completion");
    let outcome = outcomes.first().ok_or("should have the outcome")?;
    assert_eq!(
        outcome.payload.as_deref(),
        Some("the file's text"),
        "the stored result is the payload"
    );
    assert_eq!(outcome.text, "the file's text", "and the text");
    assert_eq!(outcome.status, ToolStatus::Success, "the stored verdict");
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

/// The frame rig-memory 0.47 writes around the summary a compaction splices in:
/// its `OPENS` and `CLOSES` text (rig-memory-0.47.0 src/compaction.rs:37-40).
fn rig_frame(summary: &str) -> String {
    format!(
        "The conversation history before this point was compacted into the following summary:\n\n\
         <summary>\n{summary}\n</summary>"
    )
}
