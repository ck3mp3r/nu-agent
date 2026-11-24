//! The inputs one ACP session runtime is built from.

use std::path::PathBuf;
use std::sync::Arc;

use agnt_runtime::hooks::HookAgentContext;
use agnt_runtime::session::RigStoreConfig;
use agnt_runtime::tools::mcp::config::McpServerConfig;

use crate::session::ClientFsState;

/// Inputs required to build an ACP session runtime.
///
/// Every field is resolved by the caller: the session handler owns the ACP
/// request (CWD, client MCP servers, session id) and the process-level
/// resources (tokio handle, session store).
pub struct AcpRuntimeParams {
    /// Session working directory, taken from the ACP `session/new` request.
    pub cwd: PathBuf,
    /// MCP servers declared by the ACP client. These take precedence over
    /// config.toml servers with the same name.
    pub mcp_servers: Vec<McpServerConfig>,
    /// The resolved application paths.
    ///
    /// Carries the config file and the persona directory, so the runtime and the
    /// agent selector read them instead of deriving them.
    pub paths: agnt_runtime::config::Paths,
    /// The grammar stores the session's AST tools run against.
    ///
    /// The serve entry point assembles them once for the process and the manager
    /// clones the `Arc` into every session, so a session never builds its own.
    pub grammar_stores: Arc<agnt_runtime::tools::handler::tree_sitter::GrammarStores>,
    /// Handle to the process tokio runtime.
    pub runtime_handle: tokio::runtime::Handle,
    /// The rig-memory store config the session's store is made from.
    ///
    /// The caller owns the config, so it decides where the store lives: in
    /// production it carries the store path `Paths` resolved, so no site
    /// re-derives it; a test roots the store in a tempdir with `with_cache_dir`.
    pub rig: RigStoreConfig,
    /// Persisted session id, when the session is resumed from the store.
    pub session_id: Option<String>,
    /// The ACP client's filesystem capabilities and connection, when the client
    /// advertised them at `initialize`.
    pub client_fs: Option<ClientFsState>,
    /// Agent whose permissions overlay applies to this session.
    ///
    /// When `Some`, it overrides the default agent resolved from
    /// `agents.default` in config.toml. When `None`, the configured default
    /// agent is used.
    pub agent_name: Option<String>,
    /// The agent snapshot every hook envelope of this session is stamped with.
    ///
    /// The session handler creates it and clones it into the session's hook
    /// runner. The runtime writes the model identity on construction; the
    /// startup `switch_agent` flow then writes the persona slug.
    pub hook_context: HookAgentContext,
}
