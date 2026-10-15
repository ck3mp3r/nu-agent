use super::*;

// ================================================================
// Title extraction tests
// ================================================================

/// `extract_title` takes the first user message text and returns `None` when
/// there is no usable user text.
#[test]
fn title_extraction() {
    // -- Case 1: title comes from the first user message
    let messages = vec![
        Message::user("Hello, this is my first message to the agent"),
        Message::assistant("Hi there!"),
    ];
    assert_eq!(
        extract_title(&messages),
        Some("Hello, this is my first message to the agent".to_string())
    );

    // -- Case 2: no user message at all
    assert_eq!(extract_title(&[Message::assistant("Hi there!")]), None);

    // -- Case 3: whitespace-only user message
    assert_eq!(extract_title(&[Message::user("   ")]), None);
}

/// `extract_title` truncates long text at 80 bytes, preferring a word
/// boundary and never splitting a multi-byte character.
#[test]
fn title_truncation() {
    // -- Case 1: plain truncation at 80 chars
    let messages = vec![Message::user("a".repeat(100))];
    assert_eq!(extract_title(&messages), Some("a".repeat(80)));

    // -- Case 2: truncation at the last space within 80 chars
    let long_text = "hello world ".to_string() + &"x".repeat(70) + " more text";
    let title = extract_title(&[Message::user(long_text)]);
    assert!(title.as_ref().unwrap().len() <= 80);
    assert_eq!(title, Some("hello world".to_string()));

    // -- Case 3: truncation stops at a char boundary
    // 78 ASCII bytes + a 3-byte '─' (U+2500) straddling byte 80.
    let long_text = "a".repeat(78) + "─" + &"b".repeat(10);
    assert_eq!(
        extract_title(&[Message::user(long_text)]),
        Some("a".repeat(78))
    );

    // -- Case 4: word boundary wins over a multi-byte char boundary
    let long_text = "hello world ".to_string() + &"a".repeat(66) + "─" + &"b".repeat(10);
    assert_eq!(
        extract_title(&[Message::user(long_text)]),
        Some("hello world".to_string())
    );
}

/// The extracted title survives a round trip through the filesystem store.
#[tokio::test]
async fn title_survives_round_trip_fs_store() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());

    // -- Exec & Check: a user message yields a title in list and load
    let messages = vec![
        Message::user("My first message to the agent"),
        Message::assistant("Hello! How can I help?"),
    ];
    store.create("title-test", &messages).await.unwrap();

    let sessions = store.list().await.unwrap();
    let session = sessions
        .iter()
        .find(|s| s.id == "title-test")
        .ok_or("session must be listed")?;
    assert_eq!(
        session.title,
        Some("My first message to the agent".to_string())
    );

    let (metadata, _entries) = store
        .load("title-test")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(
        metadata.title,
        Some("My first message to the agent".to_string())
    );

    // -- Exec & Check: assistant-only history yields no title
    store
        .create("no-user-msg", &[Message::assistant("Hello!")])
        .await
        .unwrap();

    let sessions = store.list().await.unwrap();
    let session = sessions
        .iter()
        .find(|s| s.id == "no-user-msg")
        .ok_or("session must be listed")?;
    assert_eq!(session.title, None);

    let (metadata, _entries) = store
        .load("no-user-msg")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;
    assert_eq!(metadata.title, None);
    Ok(())
}
