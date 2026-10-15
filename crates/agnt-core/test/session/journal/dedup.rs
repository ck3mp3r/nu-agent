use super::*;

// ---------------------------------------------------------------------------
// Integration test: no duplication when appending deltas sequentially
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_duplication_when_appending_deltas_sequentially() {
    use rig::memory::ConversationMemory;

    let temp_dir = tempfile::tempdir().unwrap();
    let memory = TestMemory::new(Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf())));
    let conversation_id = "integ-dedup-test";

    memory
        .append(
            &ConversationId::from(conversation_id),
            vec![
                crate::types::Message::user("turn1"),
                crate::types::Message::assistant("ok1"),
            ],
        )
        .await
        .expect("append turn1 should succeed");

    let entries_after_turn1 = memory.load_all(conversation_id).await.unwrap();
    assert_eq!(
        entries_after_turn1.len(),
        2,
        "after turn1: store must have exactly 2 entries"
    );

    memory
        .append(
            &ConversationId::from(conversation_id),
            vec![
                crate::types::Message::user("turn2"),
                crate::types::Message::assistant("[Turn failed: network error]"),
            ],
        )
        .await
        .expect("append turn2 delta should succeed");

    let entries_after_turn2 = memory.load_all(conversation_id).await.unwrap();
    assert_eq!(
        entries_after_turn2.len(),
        4,
        "after turn2 delta: store must have exactly 4 entries"
    );

    let after_turn2 = memory
        .load(&ConversationId::from(conversation_id))
        .await
        .expect("load should succeed");
    assert_eq!(
        after_turn2.len(),
        4,
        "after turn2 delta: load() expected 4 messages"
    );

    memory
        .append(
            &ConversationId::from(conversation_id),
            vec![
                crate::types::Message::user("turn3"),
                crate::types::Message::assistant("[Turn failed: timeout]"),
            ],
        )
        .await
        .expect("append turn3 delta should succeed");

    let entries_after_turn3 = memory.load_all(conversation_id).await.unwrap();
    assert_eq!(
        entries_after_turn3.len(),
        6,
        "after turn3 delta: store must have exactly 6 entries"
    );

    let after_turn3 = memory
        .load(&ConversationId::from(conversation_id))
        .await
        .expect("load should succeed");
    assert_eq!(
        after_turn3.len(),
        6,
        "after turn3 delta: load() expected 6 messages"
    );

    let texts: Vec<String> = after_turn3
        .iter()
        .filter_map(|msg| match msg {
            crate::types::Message::User { content } => content.iter().find_map(|c| {
                if let crate::types::UserContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            }),
            crate::types::Message::Assistant(assistant) => {
                let content = &assistant.content;
                content.iter().find_map(|c| {
                    if let crate::types::AssistantContent::Text(t) = c {
                        Some(t.text.clone())
                    } else {
                        None
                    }
                })
            }
            _ => None,
        })
        .collect();

    let unique_count = texts.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(
        unique_count,
        texts.len(),
        "no message content should appear twice; got: {:?}",
        texts
    );
}
