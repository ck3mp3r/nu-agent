//! Edit tool tests for the ACP filesystem tools.

use super::*;

// region:    --- Edit definition

#[tokio::test]
async fn test_fs_tools_make_acp_edit_tool_keeps_standard_definition() -> Result<()> {
    // -- Setup & Fixtures
    let def = edit_definition()?;
    let (tool, _recorder) = edit_tool(Reply::Content(String::new()), 20_000).await?;

    // -- Exec
    let tool_def = tool.definition();

    // -- Check
    assert_eq!(
        tool.name(),
        "edit",
        "the ACP tool must keep the standard name"
    );
    assert_eq!(tool_def.name, def.name);
    assert_eq!(tool_def.description, def.description);
    assert_eq!(tool_def.parameters, def.parameters);
    Ok(())
}

// endregion: --- Edit definition

// region:    --- Edit search_replace

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_search_replace_apply_writes() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello\nworld\n";
    let expected_version = version_token(content);
    let (tool, recorder) = edit_tool(Reply::Content(content.to_string()), 20_000).await?;

    // -- Exec
    let json = run_edit(
        &tool,
        serde_json::json!({
            "path": "test.txt",
            "mode": "apply",
            "expected_version": expected_version,
            "operation": {
                "type": "search_replace",
                "search": "world",
                "replacement": "there"
            }
        }),
    )
    .await?;

    // -- Check
    assert_eq!(json["path"], "test.txt");
    assert_eq!(json["mode"], "apply");
    assert_eq!(json["applied"], true);
    assert_eq!(json["wrote"], true);
    assert_eq!(json["changed"], true);
    assert_eq!(json["would_change"], true);
    assert_eq!(json["conflict"], false);
    assert_eq!(json["noop"], false);
    assert_eq!(json["replacements"], 1u64);
    assert_eq!(json["expected_version"], expected_version.as_str());
    assert_eq!(json["previous_version"], version_token(content));
    assert_eq!(json["new_version"], version_token("hello\nthere\n"));

    let reads = recorder.reads();
    assert_eq!(
        reads.len(),
        1,
        "exactly one fs/read_text_file request must reach the client"
    );
    assert_eq!(
        reads[0].path,
        PathBuf::from("/tmp/nu-agent-acp-edit-cwd/test.txt"),
        "a relative path must resolve against the session cwd"
    );
    assert_eq!(reads[0].session_id.to_string(), "sess-1");

    let writes = recorder.writes();
    assert_eq!(
        writes.len(),
        1,
        "exactly one fs/write_text_file request must reach the client"
    );
    assert_eq!(
        writes[0].path,
        PathBuf::from("/tmp/nu-agent-acp-edit-cwd/test.txt")
    );
    assert_eq!(writes[0].content, "hello\nthere\n");
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_stale_version_conflicts() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello\n";
    let (tool, recorder) = edit_tool(Reply::Content(content.to_string()), 20_000).await?;

    // -- Exec
    let json = run_edit(
        &tool,
        serde_json::json!({
            "path": "test.txt",
            "mode": "apply",
            "expected_version": "deadbeef",
            "operation": { "type": "search_replace", "search": "hello", "replacement": "hi" }
        }),
    )
    .await?;

    // -- Check
    assert_eq!(json["conflict"], true);
    assert_eq!(json["applied"], false);
    assert_eq!(json["expected_version"], "deadbeef");
    assert_eq!(json["previous_version"], version_token(content));
    assert!(
        !json["diagnostics"]
            .as_array()
            .ok_or("diagnostics must be an array")?
            .is_empty(),
        "a conflict must carry a stale diagnostic"
    );
    assert!(
        recorder.writes().is_empty(),
        "a version conflict must not write to the client"
    );
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_preview_does_not_write() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello\n";
    let (tool, recorder) = edit_tool(Reply::Content(content.to_string()), 20_000).await?;

    // -- Exec
    let json = run_edit(
        &tool,
        serde_json::json!({
            "path": "test.txt",
            "mode": "preview",
            "expected_version": version_token(content),
            "operation": { "type": "search_replace", "search": "hello", "replacement": "hi" }
        }),
    )
    .await?;

    // -- Check
    assert_eq!(json["mode"], "preview");
    assert_eq!(json["applied"], false);
    assert_eq!(json["would_change"], true);
    assert!(
        recorder.writes().is_empty(),
        "preview mode must not write to the client"
    );
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_read_error_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, recorder) = edit_tool(Reply::Error("boom".to_string()), 20_000).await?;

    // -- Exec
    let result = tool
        .execute(serde_json::json!({
            "path": "test.txt",
            "mode": "apply",
            "expected_version": "deadbeef",
            "operation": { "type": "search_replace", "search": "a", "replacement": "b" }
        }))
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("a client read error must fail the tool")?;
    assert!(
        err.message().contains("boom"),
        "the error must carry the client's message, got: {}",
        err.message()
    );
    assert!(
        recorder.writes().is_empty(),
        "a failed read must not write to the client"
    );
    Ok(())
}

// endregion: --- Edit search_replace

// region:    --- Edit create

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_create_apply_writes_when_missing() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, recorder) = edit_tool(Reply::Error("file not found".to_string()), 20_000).await?;

    // -- Exec
    let json = run_edit(
        &tool,
        serde_json::json!({
            "path": "new.txt",
            "mode": "apply",
            "operation": { "type": "create", "content": "fresh\n" }
        }),
    )
    .await?;

    // -- Check
    assert_eq!(json["applied"], true);
    assert_eq!(json["conflict"], false);
    assert_eq!(json["would_change"], true);
    assert_eq!(json["new_version"], version_token("fresh\n"));

    let writes = recorder.writes();
    assert_eq!(
        writes.len(),
        1,
        "exactly one fs/write_text_file request must reach the client"
    );
    assert_eq!(writes[0].content, "fresh\n");
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_create_apply_conflicts_when_exists() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, recorder) = edit_tool(Reply::Content("existing\n".to_string()), 20_000).await?;

    // -- Exec
    let json = run_edit(
        &tool,
        serde_json::json!({
            "path": "exists.txt",
            "mode": "apply",
            "operation": { "type": "create", "content": "fresh\n" }
        }),
    )
    .await?;

    // -- Check
    assert_eq!(json["applied"], false);
    assert_eq!(json["conflict"], true);
    assert_eq!(json["would_change"], false);
    assert!(
        recorder.writes().is_empty(),
        "a create conflict must not write to the client"
    );
    Ok(())
}

// endregion: --- Edit create

// region:    --- Edit response shape

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_response_has_standard_fields() -> Result<()> {
    // -- Setup & Fixtures
    let content = "hello\nworld\n";
    let (tool, _recorder) = edit_tool(Reply::Content(content.to_string()), 20_000).await?;

    // -- Exec
    let json = run_edit(
        &tool,
        serde_json::json!({
            "path": "test.txt",
            "mode": "apply",
            "expected_version": version_token(content),
            "operation": { "type": "search_replace", "search": "world", "replacement": "there" }
        }),
    )
    .await?;

    // -- Check
    let object = json
        .as_object()
        .ok_or("the edit response must be a JSON object")?;
    for field in [
        "path",
        "mode",
        "applied",
        "would_change",
        "diff",
        "stats",
        "changed",
        "replacements",
        "wrote",
        "noop",
        "conflict",
        "expected_version",
        "previous_version",
        "new_version",
    ] {
        assert!(
            object.contains_key(field),
            "the response must carry '{field}'"
        );
    }
    for field in [
        "replacements",
        "previous_bytes",
        "new_bytes",
        "previous_lines",
        "new_lines",
        "files_changed",
        "insertions",
        "deletions",
        "diff_truncated",
        "omitted_files",
        "omitted_hunks",
    ] {
        assert!(
            json["stats"].get(field).is_some(),
            "the stats block must carry '{field}'"
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_fs_tools_acp_edit_tool_invalid_args_returns_error() -> Result<()> {
    // -- Setup & Fixtures
    let (tool, recorder) = edit_tool(Reply::Content(String::new()), 20_000).await?;

    // -- Exec
    let result = tool
        .execute(serde_json::json!({ "path": "test.txt" }))
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("a missing operation must fail the tool")?;
    assert!(
        err.message().contains("Invalid edit arguments"),
        "the error must name the invalid arguments, got: {}",
        err.message()
    );
    assert!(
        recorder.reads().is_empty() && recorder.writes().is_empty(),
        "invalid arguments must not reach the client"
    );
    Ok(())
}

// endregion: --- Edit response shape
