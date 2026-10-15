//! Read tool tests for the ACP filesystem tools.

use super::*;

// region:    --- Definition

#[tokio::test]
async fn test_fs_tools_make_acp_read_tool_keeps_standard_definition() -> Result<()> {
    // -- Setup & Fixtures
    let def = read_definition()?;
    let (tool, _recorder) = read_tool(Reply::Content(String::new()), 20_000).await?;

    // -- Exec
    let tool_def = tool.definition();

    // -- Check
    assert_eq!(
        tool.name(),
        "read",
        "the ACP tool must keep the standard name"
    );
    assert_eq!(tool_def.name, def.name);
    assert_eq!(tool_def.description, def.description);
    assert_eq!(tool_def.parameters, def.parameters);
    Ok(())
}

// endregion: --- Definition

// region:    --- Request routing

#[tokio::test]
async fn test_fs_tools_acp_read_tool_returns_client_content() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello\nworld\n";
    let (tool, recorder) = read_tool(Reply::Content(content.to_string()), 20_000).await?;

    // -- Exec
    let json = run_read(&tool, serde_json::json!({ "path": "test.txt" })).await?;

    // -- Check
    assert_eq!(json["path"], "test.txt");
    assert_eq!(json["content"], content);
    assert_eq!(json["total_lines"], 2u64);
    assert_eq!(json["version"], version_token(content));
    assert_eq!(json["offset"], serde_json::Value::Null);
    assert_eq!(json["limit"], serde_json::Value::Null);

    let requests = recorder.requests();
    assert_eq!(
        requests.len(),
        1,
        "exactly one fs/read_text_file request must reach the client"
    );
    assert_eq!(
        requests[0].path,
        PathBuf::from("/tmp/nu-agent-acp-read-cwd/test.txt"),
        "a relative path must resolve against the session cwd"
    );
    assert_eq!(requests[0].session_id.to_string(), "sess-1");
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_read_tool_slices_offset_and_limit() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, _recorder) = read_tool(Reply::Content("hello\nworld\n".to_string()), 20_000).await?;

    // -- Exec
    let json = run_read(
        &tool,
        serde_json::json!({ "path": "test.txt", "offset": 0, "limit": 1 }),
    )
    .await?;

    // -- Check
    assert_eq!(json["content"], "hello\n");
    assert_eq!(json["total_lines"], 2u64);
    assert_eq!(json["offset"], 0u64);
    assert_eq!(json["limit"], 1u64);
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_read_tool_client_error_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, _recorder) = read_tool(Reply::Error("file not found".to_string()), 20_000).await?;

    // -- Exec
    let result = tool
        .execute(serde_json::json!({ "path": "missing.txt" }))
        .await;

    // -- Check
    let err = result.err().ok_or("a client error must fail the tool")?;
    assert!(
        err.message().contains("file not found"),
        "the error must carry the client's message, got: {}",
        err.message()
    );
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_read_tool_invalid_args_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, recorder) = read_tool(Reply::Content(String::new()), 20_000).await?;

    // -- Exec
    let result = tool.execute(serde_json::json!({ "offset": 0 })).await;

    // -- Check
    let err = result.err().ok_or("a missing path must fail the tool")?;
    assert!(
        err.message().contains("Invalid read arguments"),
        "the error must name the invalid arguments, got: {}",
        err.message()
    );
    assert!(
        recorder.requests().is_empty(),
        "invalid arguments must not reach the client"
    );
    Ok(())
}

// endregion: --- Request routing

// region:    --- Truncation

#[tokio::test]
async fn test_fs_tools_acp_read_tool_truncates_output() -> Result<()> {
    // -- Setup & Fixtures
    let content = "x".repeat(5_000);
    let (tool, _recorder) = read_tool(Reply::Content(content), 128).await?;

    // -- Exec
    let output = tool
        .execute(serde_json::json!({ "path": "big.txt" }))
        .await
        .map_err(|e| format!("read tool must succeed: {e}"))?;
    let text = output
        .as_text()
        .ok_or("read tool output must be one plain text block")?;

    // -- Check
    assert!(
        text.contains("[output truncated:"),
        "an oversized payload must be truncated, got: {text}"
    );
    assert!(
        text.starts_with("{\"path\":\"big.txt\""),
        "truncation must keep the head of the JSON payload, got: {text}"
    );
    cleanup_truncated_file(text);
    Ok(())
}

// endregion: --- Truncation
