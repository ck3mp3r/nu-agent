//! Shared fixtures for the ACP filesystem tool test modules.
//!
//! Holds the imports and the helper functions every topical fs_tools test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use std::path::PathBuf;
pub(super) use std::sync::{Arc, Mutex};

pub(super) use agent_client_protocol::schema::v1::{
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
};
pub(super) use agent_client_protocol::{Agent, Channel, Client, ConnectionTo, on_receive_request};
pub(super) use agnt_core::conversation::builder::builtin_tool_definitions;
pub(super) use agnt_core::tools::fs::core::version_token;
pub(super) use agnt_core::types::{
    DynamicTool, ToolContext, ToolDefinition, ToolExecutionError, ToolOutput, ToolSet,
};

pub(super) use crate::fs_tools::{make_acp_edit_tool, make_acp_read_tool};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Fixtures

/// The standard `read` tool definition, as the runtime hands it to the ACP tool.
pub(super) fn read_definition() -> Result<ToolDefinition> {
    builtin_tool_definitions()?
        .into_iter()
        .find(|def| def.name == "read")
        .ok_or_else(|| "builtin_tool_definitions must contain a 'read' tool".into())
}

/// What the fake client answers a `fs/read_text_file` request with.
pub(super) enum Reply {
    Content(String),
    Error(String),
}

/// Records the `fs/read_text_file` requests the tool sends.
#[derive(Default)]
pub(super) struct ReadRecorder {
    requests: Mutex<Vec<ReadTextFileRequest>>,
}

impl ReadRecorder {
    pub(super) fn requests(&self) -> Vec<ReadTextFileRequest> {
        self.requests.lock().unwrap().clone()
    }
}

/// A live in-memory `ConnectionTo<Client>` plus a client that answers every
/// `fs/read_text_file` request with `reply`.
///
/// The connection runs over a duplex channel: no transport, no network, no
/// subprocess. The client task owns the peer endpoint, so the channel stays
/// open for the test's lifetime.
pub(super) async fn read_client(reply: Reply) -> Result<(ConnectionTo<Client>, Arc<ReadRecorder>)> {
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
pub(super) async fn read_tool(
    reply: Reply,
    max_bytes: usize,
) -> Result<(DynamicTool, Arc<ReadRecorder>)> {
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
pub(super) async fn run_read(
    tool: &DynamicTool,
    args: serde_json::Value,
) -> Result<serde_json::Value> {
    let output = tool
        .execute(args)
        .await
        .map_err(|e| format!("read tool must succeed: {e}"))?;
    let text = output
        .as_text()
        .ok_or("read tool output must be one plain text block")?;
    Ok(serde_json::from_str(text)?)
}

/// `DynamicTool::execute` for rig 0.45, which removed it from the type.
///
/// A dynamic tool runs through the `ToolSet` that holds it, so this adapter
/// keeps the tests' `tool.execute(args)` call shape.
pub(super) trait DynamicToolExec {
    fn execute(
        &self,
        args: serde_json::Value,
    ) -> impl std::future::Future<Output = core::result::Result<ToolOutput, ToolExecutionError>>;
}

impl DynamicToolExec for DynamicTool {
    fn execute(
        &self,
        args: serde_json::Value,
    ) -> impl std::future::Future<Output = core::result::Result<ToolOutput, ToolExecutionError>>
    {
        let tool = self.clone();
        async move {
            let mut toolset = ToolSet::default();
            toolset.add_dynamic_tool(tool.clone());
            let mut context = ToolContext::new();
            let result = toolset
                .execute(tool.name().as_str(), args.to_string(), &mut context)
                .await;
            match result.error() {
                Some(error) => Err(error.clone()),
                None => Ok(result.output().clone()),
            }
        }
    }
}

/// Remove the temp file `truncate_tool_output` wrote, so the test leaves no trace.
pub(super) fn cleanup_truncated_file(output: &str) {
    let Some(rest) = output.split("Full output saved to: ").nth(1) else {
        return;
    };
    let Some(path) = rest.split(". Use the").next() else {
        return;
    };
    std::fs::remove_file(path).ok();
}

/// The standard `edit` tool definition, as the runtime hands it to the ACP tool.
pub(super) fn edit_definition() -> Result<ToolDefinition> {
    builtin_tool_definitions()?
        .into_iter()
        .find(|def| def.name == "edit")
        .ok_or_else(|| "builtin_tool_definitions must contain an 'edit' tool".into())
}

/// Records the `fs/read_text_file` and `fs/write_text_file` requests the tool sends.
#[derive(Default)]
pub(super) struct EditRecorder {
    read_requests: Mutex<Vec<ReadTextFileRequest>>,
    write_requests: Mutex<Vec<WriteTextFileRequest>>,
}

impl EditRecorder {
    pub(super) fn reads(&self) -> Vec<ReadTextFileRequest> {
        self.read_requests.lock().unwrap().clone()
    }

    pub(super) fn writes(&self) -> Vec<WriteTextFileRequest> {
        self.write_requests.lock().unwrap().clone()
    }
}

/// A live in-memory `ConnectionTo<Client>` plus a client that answers every
/// `fs/read_text_file` request with `reply` and every `fs/write_text_file`
/// request with an empty success response.
pub(super) async fn edit_client(reply: Reply) -> Result<(ConnectionTo<Client>, Arc<EditRecorder>)> {
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
pub(super) async fn edit_tool(
    reply: Reply,
    max_bytes: usize,
) -> Result<(DynamicTool, Arc<EditRecorder>)> {
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
pub(super) async fn run_edit(
    tool: &DynamicTool,
    args: serde_json::Value,
) -> Result<serde_json::Value> {
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
