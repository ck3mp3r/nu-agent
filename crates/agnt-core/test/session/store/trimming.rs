use super::*;

// --- Orphan user message trimming tests ---

/// `extract_llm_context` trims trailing orphan user messages and leaves a
/// valid conversation ending with an assistant message untouched.
#[test]
fn extract_llm_context_trims_orphan_user_messages() {
    // -- Case 1: a single trailing orphan user message is trimmed
    let entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Message(Message::assistant("reply")),
        StoreEntry::Message(Message::user("orphan")),
    ];
    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 2);
    assert!(matches!(&context[0], Message::User { .. }));
    assert!(matches!(&context[1], Message::Assistant(_)));

    // -- Case 2: stacked trailing orphan user messages are all trimmed
    let entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Message(Message::assistant("reply")),
        StoreEntry::Message(Message::user("orphan1")),
        StoreEntry::Message(Message::user("orphan2")),
        StoreEntry::Message(Message::user("orphan3")),
    ];
    let context = extract_llm_context(&entries);
    assert_eq!(context.len(), 2);
    assert!(matches!(&context[0], Message::User { .. }));
    assert!(matches!(&context[1], Message::Assistant(_)));

    // -- Case 3: a conversation ending with an assistant message is unchanged
    let entries = vec![
        StoreEntry::Message(Message::user("first")),
        StoreEntry::Message(Message::assistant("reply1")),
        StoreEntry::Message(Message::user("second")),
        StoreEntry::Message(Message::assistant("reply2")),
    ];
    assert_eq!(extract_llm_context(&entries).len(), 4);

    // -- Case 4: a lone orphan user message trims to empty
    let entries = vec![StoreEntry::Message(Message::user("orphan"))];
    assert_eq!(extract_llm_context(&entries).len(), 0);
}

// --- Post-compaction entry ordering tests ---

/// `load` preserves marker and post-marker entries in order.
#[tokio::test]
async fn load_preserves_post_compaction_entries() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // -- Exec & Check: [msg, marker, msg] loads in order
    store
        .create("s_post_marker", &[Message::user("before compaction")])
        .await
        .unwrap();
    let marker = CompactionMarker::new("summary".to_string(), Utc::now());
    store
        .append("s_post_marker", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();
    store
        .append(
            "s_post_marker",
            &[StoreEntry::Message(Message::assistant("kept"))],
        )
        .await
        .unwrap();

    let (_metadata, entries) = store
        .load("s_post_marker")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 3); // original msg + marker + kept msg
    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Marker(_)));
    assert!(matches!(&entries[2], StoreEntry::Message(_)));

    // -- Exec & Check: [msg, marker, msg, msg] loads in order
    store
        .create("s_post_compact_turn", &[Message::user("pre-compact")])
        .await
        .unwrap();
    let marker = CompactionMarker::new("summary".to_string(), Utc::now());
    store
        .append("s_post_compact_turn", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();
    store
        .append(
            "s_post_compact_turn",
            &[StoreEntry::Message(Message::assistant("kept"))],
        )
        .await
        .unwrap();
    store
        .append(
            "s_post_compact_turn",
            &[StoreEntry::Message(Message::user("post-compact turn"))],
        )
        .await
        .unwrap();

    let (_metadata, entries) = store
        .load("s_post_compact_turn")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(entries.len(), 4);
    assert!(matches!(&entries[0], StoreEntry::Message(_)));
    assert!(matches!(&entries[1], StoreEntry::Marker(_)));
    assert!(matches!(&entries[2], StoreEntry::Message(_)));
    assert!(matches!(&entries[3], StoreEntry::Message(_)));
    Ok(())
}
