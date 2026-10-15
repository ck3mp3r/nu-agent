use super::*;

// ================================================================
// Title extraction tests
// ================================================================

#[tokio::test]
async fn title_extracted_from_first_user_message_sqlite() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let messages = vec![
        Message::user("Hello, this is my first message"),
        Message::assistant("Hi there!"),
    ];
    store
        .create("title-test-1", &messages)
        .await
        .expect("create");

    // Verify via list
    let sessions = store.list().await.expect("list");
    let session = sessions.iter().find(|s| s.id == "title-test-1").unwrap();
    assert_eq!(
        session.title,
        Some("Hello, this is my first message".to_string())
    );

    // Verify via load
    let (metadata, _entries) = store
        .load("title-test-1")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(
        metadata.title,
        Some("Hello, this is my first message".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn title_none_when_no_user_message_sqlite() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let messages = vec![Message::assistant("Hello!")];
    store
        .create("no-user-msg-sqlite", &messages)
        .await
        .expect("create");

    let sessions = store.list().await.expect("list");
    let session = sessions
        .iter()
        .find(|s| s.id == "no-user-msg-sqlite")
        .unwrap();
    assert_eq!(session.title, None);

    let (metadata, _entries) = store
        .load("no-user-msg-sqlite")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, None);
    Ok(())
}

#[tokio::test]
async fn title_survives_round_trip_sqlite() -> Result<()> {
    let store = SqliteSessionStore::new(":memory:")
        .await
        .expect("create store");

    let messages = vec![
        Message::user("Round trip title test"),
        Message::assistant("Response"),
    ];
    store.create("round-trip", &messages).await.expect("create");

    // list → load
    let sessions = store.list().await.expect("list");
    let session = sessions.iter().find(|s| s.id == "round-trip").unwrap();
    assert_eq!(session.title, Some("Round trip title test".to_string()));

    let (metadata, _entries) = store
        .load("round-trip")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, Some("Round trip title test".to_string()));
    Ok(())
}

#[tokio::test]
async fn title_multibyte_utf8_straddling_byte_80_sqlite() -> Result<()> {
    // -- Setup & Fixtures
    let store = SqliteSessionStore::new(":memory:")
        .await
        .map_err(|e| format!("create store should succeed: {e:?}"))?;

    // 79 ASCII bytes + a 2-byte char whose first byte lands at offset 80,
    // then more text to push the message past 80 bytes total.
    let long = format!("{}é rest of the message", "a".repeat(79));
    let messages = vec![Message::user(long), Message::assistant("Hi")];

    // -- Exec
    store
        .create("utf8-title-sqlite", &messages)
        .await
        .map_err(|e| format!("create should succeed: {e:?}"))?;

    // -- Check
    let expected = "a".repeat(79);
    let sessions = store
        .list()
        .await
        .map_err(|e| format!("list should succeed: {e:?}"))?;
    let session = sessions
        .iter()
        .find(|s| s.id == "utf8-title-sqlite")
        .ok_or("session utf8-title-sqlite should be in list")?;
    assert_eq!(session.title, Some(expected.clone()));

    let (metadata, _entries) = store
        .load("utf8-title-sqlite")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, Some(expected));
    Ok(())
}
