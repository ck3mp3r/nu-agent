//! Adapters between the ACP wire schema and the crate's own types.
//!
//! The ACP handlers call these: the identity and capabilities the agent
//! advertises, the flattening of a request into crate operations, and the
//! translation of a crate error into the error the client sees.

// region:    --- Modules

use std::collections::HashMap;

use agent_client_protocol::Error;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, Implementation, McpServer, SessionCapabilities,
    SessionDeleteCapabilities, SessionListCapabilities, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse,
};
use agnt_runtime::tools::mcp::config::{McpAuthConfig, McpServerConfig, McpTransportType};

use crate::runtime::{AGENT_CONFIG_ID, MODEL_CONFIG_ID};
use crate::{AcpError, AcpSessionManager, Result, RuntimeBuilder};

// endregion: --- Modules

// region:    --- Functions

pub(crate) fn agent_info() -> Implementation {
    Implementation::new("nu-agent", env!("CARGO_PKG_VERSION")).title("nu-agent")
}

/// Capabilities advertised at `initialize`.
///
/// `load_session` plus the `list` and `delete` session capabilities are what
/// make `session/load`, `session/list`, and `session/delete` reachable: a
/// conformant client only calls them when the agent advertises support.
pub(crate) fn agent_capabilities() -> AgentCapabilities {
    AgentCapabilities::new()
        .load_session(true)
        .session_capabilities(
            SessionCapabilities::new()
                .list(SessionListCapabilities::default())
                .delete(SessionDeleteCapabilities::default()),
        )
}

/// Map an internal error onto the ACP error the client sees.
pub(crate) fn to_acp_error(err: AcpError) -> Error {
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
/// The model and agent selectors are supported; any other config id is
/// rejected. The requested value must be one of the advertised values of the
/// addressed selector.
pub(crate) async fn handle_set_config_option<R: RuntimeBuilder>(
    sessions: &AcpSessionManager<R>,
    request: SetSessionConfigOptionRequest,
) -> Result<SetSessionConfigOptionResponse> {
    let config_id = request.config_id.to_string();
    if config_id != MODEL_CONFIG_ID && config_id != AGENT_CONFIG_ID {
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
    let session_id = request.session_id.to_string();
    let options = if config_id == AGENT_CONFIG_ID {
        sessions.set_agent(&session_id, &value).await?
    } else {
        sessions.set_model(&session_id, &value).await?
    };
    Ok(SetSessionConfigOptionResponse::new(options))
}

/// Flatten the prompt's content blocks into the text the turn runs on.
pub(crate) fn prompt_text(blocks: &[ContentBlock]) -> String {
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
pub(crate) fn mcp_server_config(server: &McpServer) -> Option<McpServerConfig> {
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

// endregion: --- Functions
