use super::*;

// --- AssistantContent tagged serialization (rig 0.42.0) ---

/// rig 0.42.0 serializes `AssistantContent` with a `"type"` tag
/// (`#[serde(tag = "type", rename_all = "lowercase")]`). A
/// `Message::Assistant` round-trips with the tag intact, and a bare tagless
/// block is rejected.
#[test]
fn assistant_content_serde() -> TestResult<()> {
    use crate::types::{AssistantContent, CallId, ToolCall, ToolFunction, ToolName};

    // -- Setup & Fixtures
    let msg = crate::types::Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::Text(crate::types::Text::new("hello")),
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_1"),
            ToolFunction::new(
                ToolName::new("some_tool").map_err(|_| "non-empty tool name")?,
                serde_json::json!({"a": 1}),
            ),
        )),
    ]));

    // -- Exec
    let value = serde_json::to_value(&msg).unwrap();
    let round_tripped: crate::types::Message = serde_json::from_value(value.clone()).unwrap();
    let tagless: Result<crate::types::AssistantContent, _> =
        serde_json::from_value(serde_json::json!({"text": "hello"}));

    // -- Check
    let content = value["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    // `rename_all = "lowercase"` lowercases the variant name without inserting
    // separators, so `ToolCall` serializes as `"toolcall"`.
    assert_eq!(content[1]["type"], "toolcall");
    assert_eq!(content[1]["function"]["name"], "some_tool");
    assert_msg_eq(&round_tripped, &msg);
    assert!(tagless.is_err(), "tagless block must be rejected");
    Ok(())
}

// --- StoreEntry serialization tests ---

/// `append` writes messages and markers to the raw JSONL with the expected
/// shape.
#[tokio::test]
async fn append_writes_entries_to_jsonl() {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    store.create("s1", &[Message::user("hi")]).await.unwrap();

    // -- Exec & Check: a message line carries the role
    let content = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "line 0 = metadata, line 1 = message");
    let value: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(value["role"], "user");

    // -- Exec & Check: a marker line carries the type tag
    let marker = CompactionMarker::new("summary".to_string(), Utc::now());
    store
        .append("s1", &[StoreEntry::Marker(marker)])
        .await
        .unwrap();

    let content = std::fs::read_to_string(temp_dir.path().join("s1.jsonl")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3, "line 2 = marker");
    let value: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(value["type"], "compaction_marker");
}

/// `load` returns every entry after multiple appends.
#[tokio::test]
async fn load_returns_all_entries_after_multiple_appends() -> TestResult<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().unwrap();
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let msgs = vec![Message::user("hi"), Message::assistant("there")];

    // -- Exec
    store.create("s1", &msgs).await.unwrap();
    store
        .append("s1", &[StoreEntry::Message(Message::assistant("hello"))])
        .await
        .unwrap();
    let (_metadata, entries) = store
        .load("s1")
        .await
        .map_err(|e| format!("{e:?}"))?
        .ok_or("should be some")?;

    // -- Check
    assert_eq!(entries.len(), 3);
    Ok(())
}
