//! ACP (Agent Client Protocol) stdio server for nu-agent.

// region:    --- Modules

pub mod engine_stub;
pub mod error;
pub mod fs_tools;
pub mod permission;
pub mod runtime;
pub mod session;
pub mod translator;

pub use engine_stub::*;
pub use error::*;
pub use fs_tools::*;
pub use permission::*;
pub use runtime::*;
pub use session::*;
pub use translator::*;

// endregion: --- Modules

// region:    --- Types

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock,
    DeleteSessionRequest, DeleteSessionResponse, Implementation, InitializeRequest,
    InitializeResponse, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, McpServer,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, ResumeSessionRequest,
    ResumeSessionResponse, SessionCapabilities, SessionDeleteCapabilities, SessionId,
    SessionInfo as AcpSessionInfo, SessionListCapabilities, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse,
};
use agent_client_protocol::{Agent, Error, Stdio, on_receive_notification, on_receive_request};
use agnt_core::config::{ModelsCache, toml_config};
use agnt_core::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};

/// The session registry shared by every protocol handler.
///
/// The registry is shared by `Arc` alone: every method takes `&self` and holds
/// its own internal lock only for a lookup, so a long-running turn cannot block
/// `session/cancel`.
type SharedSessions = Arc<AcpSessionManager<DefaultRuntimeBuilder>>;

// endregion: --- Types

// region:    --- Public Functions

/// Serve the ACP protocol over stdio until the client disconnects.
pub async fn serve() -> Result<()> {
    let config_path = toml_config::config_path()
        .map_err(|e| AcpError::Config(format!("failed to resolve config path: {e}")))?;
    let store = create_acp_store().await?;
    let models_cache = match ModelsCache::load() {
        Ok(cache) => Some(Arc::new(cache)),
        Err(err) => {
            log::debug!("models cache unavailable, advertising only the configured model: {err}");
            None
        }
    };
    let client_fs: Arc<StdMutex<Option<ClientFsState>>> = Arc::new(StdMutex::new(None));
    let sessions: SharedSessions = Arc::new(
        AcpSessionManager::new(store, config_path, tokio::runtime::Handle::current())
            .with_models_cache(models_cache)
            .with_client_fs(Arc::clone(&client_fs)),
    );

    Agent
        .builder()
        .name("nu-agent")
        .on_receive_request(
            {
                let client_fs = Arc::clone(&client_fs);
                async move |request: InitializeRequest, responder, connection| {
                    *client_fs.lock().unwrap() = Some(ClientFsState {
                        connection,
                        capabilities: request.client_capabilities.fs.clone(),
                    });
                    responder.respond(
                        InitializeResponse::new(request.protocol_version)
                            .agent_capabilities(agent_capabilities())
                            .agent_info(agent_info()),
                    )
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: NewSessionRequest, responder, _connection| {
                    let mcp_servers = request
                        .mcp_servers
                        .iter()
                        .filter_map(mcp_server_config)
                        .collect();
                    let result = sessions.new_session(request.cwd, mcp_servers).await;
                    match result {
                        Ok(session_id) => match sessions.model_config_options(&session_id).await {
                            Ok(options) => responder.respond(
                                NewSessionResponse::new(SessionId::new(session_id))
                                    .config_options(options),
                            ),
                            Err(err) => responder.respond_with_error(to_acp_error(err)),
                        },
                        Err(err) => responder.respond_with_error(to_acp_error(err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: SetSessionConfigOptionRequest, responder, _connection| {
                    match handle_set_config_option(&sessions, request).await {
                        Ok(response) => responder.respond(response),
                        Err(err) => responder.respond_with_error(to_acp_error(err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: PromptRequest, responder, connection| {
                    let session_id = request.session_id.to_string();
                    let prompt_text = prompt_text(&request.prompt);
                    // The turn runs in a spawned task. The dispatch loop must stay
                    // free to deliver `session/cancel` and the client's
                    // `session/request_permission` response while the turn is in
                    // flight; awaiting the turn here would deadlock both.
                    let turn_sessions = Arc::clone(&sessions);
                    let turn_connection = connection.clone();
                    connection.spawn(async move {
                        match turn_sessions
                            .prompt(&session_id, prompt_text, &turn_connection)
                            .await
                        {
                            Ok(stop_reason) => responder.respond(PromptResponse::new(stop_reason)),
                            Err(err) => responder.respond_with_error(to_acp_error(err)),
                        }
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: LoadSessionRequest, responder, connection| {
                    let result = sessions
                        .load_session(&request.session_id.to_string(), request.cwd, &connection)
                        .await;
                    match result {
                        Ok(response) => responder.respond(response),
                        Err(err) => responder.respond_with_error(to_acp_error(err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: ResumeSessionRequest, responder, _connection| {
                    let result = sessions
                        .resume_session(&request.session_id.to_string(), request.cwd)
                        .await;
                    match result {
                        Ok(_) => responder.respond(ResumeSessionResponse::new()),
                        Err(err) => responder.respond_with_error(to_acp_error(err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: CloseSessionRequest, responder, _connection| {
                    let result = sessions.close_session(&request.session_id.to_string());
                    match result {
                        Ok(()) => responder.respond(CloseSessionResponse::new()),
                        Err(err) => responder.respond_with_error(to_acp_error(err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |_request: ListSessionsRequest, responder, _connection| {
                    let listed = sessions
                        .list_sessions()
                        .into_iter()
                        .filter_map(|info| {
                            let cwd = sessions.get_session(&info.id)?.cwd().to_path_buf();
                            Some(AcpSessionInfo::new(SessionId::new(info.id), cwd))
                        })
                        .collect();
                    responder.respond(ListSessionsResponse::new(listed))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |request: DeleteSessionRequest, responder, _connection| {
                    let result = sessions
                        .delete_session(&request.session_id.to_string())
                        .await;
                    match result {
                        Ok(()) => responder.respond(DeleteSessionResponse::new()),
                        Err(err) => responder.respond_with_error(to_acp_error(err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            {
                let sessions = Arc::clone(&sessions);
                async move |notification: CancelNotification, _connection| {
                    let result = sessions.cancel(&notification.session_id.to_string()).await;
                    if let Err(err) = result {
                        log::warn!("session/cancel failed: {err}");
                    }
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await?;

    Ok(())
}

// endregion: --- Public Functions

// region:    --- Support

fn agent_info() -> Implementation {
    Implementation::new("nu-agent", env!("CARGO_PKG_VERSION")).title("nu-agent")
}

/// Capabilities advertised at `initialize`.
///
/// `load_session` plus the `list` and `delete` session capabilities are what
/// make `session/load`, `session/list`, and `session/delete` reachable: a
/// conformant client only calls them when the agent advertises support.
fn agent_capabilities() -> AgentCapabilities {
    AgentCapabilities::new()
        .load_session(true)
        .session_capabilities(
            SessionCapabilities::new()
                .list(SessionListCapabilities::default())
                .delete(SessionDeleteCapabilities::default()),
        )
}

/// Map an internal error onto the ACP error the client sees.
fn to_acp_error(err: AcpError) -> Error {
    match err {
        AcpError::SessionNotFound(id) => {
            Error::invalid_params().data(format!("session not found: {id}"))
        }
        AcpError::SessionAlreadyExists(id) => {
            Error::invalid_params().data(format!("session already exists: {id}"))
        }
        AcpError::InvalidSessionState(message) => Error::invalid_params().data(message),
        AcpError::Config(message) => {
            Error::internal_error().data(format!("configuration error: {message}"))
        }
        AcpError::Runtime(message) => {
            Error::internal_error().data(format!("runtime error: {message}"))
        }
    }
}

/// Apply one `session/set_config_option` request and build its response.
///
/// Only the model selector is supported; any other config id is rejected.
/// The requested value must be one of the advertised model values.
async fn handle_set_config_option<R: RuntimeBuilder>(
    sessions: &AcpSessionManager<R>,
    request: SetSessionConfigOptionRequest,
) -> Result<SetSessionConfigOptionResponse> {
    if request.config_id.to_string() != MODEL_CONFIG_ID {
        return Err(AcpError::Config(format!(
            "unknown config option '{}'",
            request.config_id
        )));
    }
    let value = request
        .value
        .as_value_id()
        .map(|id| id.to_string())
        .ok_or_else(|| AcpError::Config("config option value must be a string id".to_string()))?;
    let options = sessions
        .set_model(&request.session_id.to_string(), &value)
        .await?;
    Ok(SetSessionConfigOptionResponse::new(options))
}

/// Flatten the prompt's content blocks into the text the turn runs on.
fn prompt_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert one ACP MCP server declaration into a nu-agent MCP server config.
///
/// Returns `None` for transports nu-agent cannot speak, so an unknown variant
/// is skipped rather than failing the whole `session/new`.
fn mcp_server_config(server: &McpServer) -> Option<McpServerConfig> {
    match server {
        McpServer::Http(http) => Some(McpServerConfig {
            name: http.name.clone(),
            transport: McpTransportType::Http,
            enabled: true,
            url: Some(http.url.clone()),
            auth: McpAuthConfig::None,
            headers: http
                .headers
                .iter()
                .map(|header| (header.name.clone(), header.value.clone()))
                .collect(),
            command: None,
            cwd: None,
            args: Vec::new(),
            env: HashMap::new(),
        }),
        McpServer::Sse(sse) => Some(McpServerConfig {
            name: sse.name.clone(),
            transport: McpTransportType::Sse,
            enabled: true,
            url: Some(sse.url.clone()),
            auth: McpAuthConfig::None,
            headers: sse
                .headers
                .iter()
                .map(|header| (header.name.clone(), header.value.clone()))
                .collect(),
            command: None,
            cwd: None,
            args: Vec::new(),
            env: HashMap::new(),
        }),
        McpServer::Stdio(stdio) => Some(McpServerConfig {
            name: stdio.name.clone(),
            transport: McpTransportType::Stdio,
            enabled: true,
            url: None,
            auth: McpAuthConfig::None,
            headers: HashMap::new(),
            command: Some(stdio.command.to_string_lossy().to_string()),
            cwd: None,
            args: stdio.args.clone(),
            env: stdio
                .env
                .iter()
                .map(|variable| (variable.name.clone(), variable.value.clone()))
                .collect(),
        }),
        _ => None,
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/error.rs"]
mod error_test;

#[cfg(test)]
#[path = "../test/permission.rs"]
mod permission_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/integration.rs"]
mod integration_test;

// endregion: --- Tests
