//! Unit tests for the ACP filesystem tools.
//!
//! Every test drives `make_acp_read_tool` or `make_acp_edit_tool` against an
//! in-memory ACP client: the tool's `fs/read_text_file` and `fs/write_text_file`
//! requests travel over a duplex channel, so no transport, no network, and no
//! subprocess is involved.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo, on_receive_request};
use agnt_core::conversation::builder::builtin_tool_definitions;
use agnt_core::tools::fs::core::version_token;
use agnt_core::types::{DynamicTool, ToolDefinition};

use crate::fs_tools::{make_acp_edit_tool, make_acp_read_tool};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The standard `read` tool definition, as the runtime hands it to the ACP tool.
fn read_definition() -> Result<ToolDefinition> {
    builtin_tool_definitions()
        .into_iter()
        .find(|def| def.name == "read")
        .ok_or_else(|| "builtin_tool_definitions must contain a 'read' tool".into())
}

/// What the fake client answers a `fs/read_text_file` request with.
enum Reply {
    Content(String),
    Error(String),
}

/// Records the `fs/read_text_file` requests the tool sends.
#[derive(Default)]
struct ReadRecorder {
    requests: Mutex<Vec<ReadTextFileRequest>>,
}

impl ReadRecorder {
    fn requests(&self) -> Vec<ReadTextFileRequest> {
        self.requests.lock().unwrap().clone()
    }
}

/// A live in-memory `ConnectionTo<Client>` plus a client that answers every
/// `fs/read_text_file` request with `reply`.
///
/// The connection runs over a duplex channel: no transport, no network, no
/// subprocess. The client task owns the peer endpoint, so the channel stays
/// open for the test's lifetime.
async fn read_client(reply: Reply) -> Result<(ConnectionTo<Client>, Arc<ReadRecorder>)> {
    let (agent_channel, client_channel) = Channel::duplex();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = Agent
            .builder()
            .connect_with(agent_channel, async move |cx| {
                let _ = tx.send(cx);
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
    });
    let connection = rx.await?;

    let recorder = Arc::new(ReadRecorder::default());
    let client_recorder = Arc::clone(&recorder);
    let reply = Arc::new(reply);
    tokio::spawn(async move {
        let _ = Client
            .builder()
            .name("agnt-acp-fs-test-client")
            .on_receive_request(
                async move |request: ReadTextFileRequest, responder, _cx| {
                    client_recorder.requests.lock().unwrap().push(request);
                    match reply.as_ref() {
                        Reply::Content(content) => {
                            responder.respond(ReadTextFileResponse::new(content.clone()))
                        }
                        Reply::Error(message) => responder.respond_with_error(acp_err(message)),
                    }
                },
                on_receive_request!(),
            )
            .connect_to(client_channel)
            .await;
    });

    Ok((connection, recorder))
}

/// Wrap a message as an ACP internal error so it can cross the client closure.
fn acp_err(message: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(message.to_string())
}

/// Build the ACP read tool over a fresh in-memory client.
async fn read_tool(reply: Reply, max_bytes: usize) -> Result<(DynamicTool, Arc<ReadRecorder>)> {
    let (connection, recorder) = read_client(reply).await?;
    let tool = make_acp_read_tool(
        read_definition()?,
        PathBuf::from("/tmp/nu-agent-acp-read-cwd"),
        max_bytes,
        connection,
        "sess-1".to_string(),
    );
    Ok((tool, recorder))
}

/// Run the tool and parse its JSON payload.
async fn run_read(tool: &DynamicTool, args: serde_json::Value) -> Result<serde_json::Value> {
    let output = tool
        .execute(args)
        .await
        .map_err(|e| format!("read tool must succeed: {e}"))?;
    let text = output
        .as_text()
        .ok_or("read tool output must be one plain text block")?;
    Ok(serde_json::from_str(text)?)
}

/// Remove the temp file `truncate_tool_output` wrote, so the test leaves no trace.
fn cleanup_truncated_file(output: &str) {
    let Some(rest) = output.split("Full output saved to: ").nth(1) else {
        return;
    };
    let Some(path) = rest.split(". Use the").next() else {
        return;
    };
    std::fs::remove_file(path).ok();
}

/// The standard `edit` tool definition, as the runtime hands it to the ACP tool.
fn edit_definition() -> Result<ToolDefinition> {
    builtin_tool_definitions()
        .into_iter()
        .find(|def| def.name == "edit")
        .ok_or_else(|| "builtin_tool_definitions must contain an 'edit' tool".into())
}

/// Records the `fs/read_text_file` and `fs/write_text_file` requests the tool sends.
#[derive(Default)]
struct EditRecorder {
    read_requests: Mutex<Vec<ReadTextFileRequest>>,
    write_requests: Mutex<Vec<WriteTextFileRequest>>,
}

impl EditRecorder {
    fn reads(&self) -> Vec<ReadTextFileRequest> {
        self.read_requests.lock().unwrap().clone()
    }

    fn writes(&self) -> Vec<WriteTextFileRequest> {
        self.write_requests.lock().unwrap().clone()
    }
}

/// A live in-memory `ConnectionTo<Client>` plus a client that answers every
/// `fs/read_text_file` request with `reply` and every `fs/write_text_file`
/// request with an empty success response.
async fn edit_client(reply: Reply) -> Result<(ConnectionTo<Client>, Arc<EditRecorder>)> {
    let (agent_channel, client_channel) = Channel::duplex();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = Agent
            .builder()
            .connect_with(agent_channel, async move |cx| {
                let _ = tx.send(cx);
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
    });
    let connection = rx.await?;

    let recorder = Arc::new(EditRecorder::default());
    let read_recorder = Arc::clone(&recorder);
    let write_recorder = Arc::clone(&recorder);
    let reply = Arc::new(reply);
    tokio::spawn(async move {
        let _ = Client
            .builder()
            .name("agnt-acp-fs-edit-test-client")
            .on_receive_request(
                async move |request: ReadTextFileRequest, responder, _cx| {
                    read_recorder.read_requests.lock().unwrap().push(request);
                    match reply.as_ref() {
                        Reply::Content(content) => {
                            responder.respond(ReadTextFileResponse::new(content.clone()))
                        }
                        Reply::Error(message) => responder.respond_with_error(acp_err(message)),
                    }
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |request: WriteTextFileRequest, responder, _cx| {
                    write_recorder.write_requests.lock().unwrap().push(request);
                    responder.respond(WriteTextFileResponse::new())
                },
                on_receive_request!(),
            )
            .connect_to(client_channel)
            .await;
    });

    Ok((connection, recorder))
}

/// Build the ACP edit tool over a fresh in-memory client.
async fn edit_tool(reply: Reply, max_bytes: usize) -> Result<(DynamicTool, Arc<EditRecorder>)> {
    let (connection, recorder) = edit_client(reply).await?;
    let tool = make_acp_edit_tool(
        edit_definition()?,
        PathBuf::from("/tmp/nu-agent-acp-edit-cwd"),
        max_bytes,
        connection,
        "sess-1".to_string(),
    );
    Ok((tool, recorder))
}

/// Run the edit tool and parse its JSON payload.
async fn run_edit(tool: &DynamicTool, args: serde_json::Value) -> Result<serde_json::Value> {
    let output = tool
        .execute(args)
        .await
        .map_err(|e| format!("edit tool must succeed: {e}"))?;
    let text = output
        .as_text()
        .ok_or("edit tool output must be one plain text block")?;
    Ok(serde_json::from_str(text)?)
}

// endregion: --- Fixtures

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
