//! The ACP stdio server: connection setup, protocol handlers, and shutdown.

// region:    --- Modules

use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema::v1::{
    CancelNotification, CloseSessionRequest, CloseSessionResponse, DeleteSessionRequest,
    DeleteSessionResponse, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionId,
    SessionInfo as AcpSessionInfo, SetSessionConfigOptionRequest,
};
use agent_client_protocol::{Agent, Stdio, on_receive_notification, on_receive_request};
use agnt_runtime::bus::{AgentEvent, AgentTx};
use agnt_runtime::config::{AgentsConfig, ModelsCache, PluginConfig, toml_config};
use agnt_runtime::hooks::HookAgentContext;
use agnt_runtime::protocol::persona::{FsPersonaResolver, PersonaFileResolver};

use crate::{
    AcpError, AcpSessionManager, ClientFsState, DefaultRuntimeBuilder, Result, agent_capabilities,
    agent_info, handle_set_config_option, mcp_server_config, prompt_text, to_acp_error,
};

// endregion: --- Modules

// region:    --- Types

/// The session registry shared by every protocol handler.
///
/// The registry is shared by `Arc` alone: every method takes `&self` and holds
/// its own internal lock only for a lookup, so a long-running turn cannot block
/// `session/cancel`.
type SharedSessions = Arc<AcpSessionManager<DefaultRuntimeBuilder>>;

// endregion: --- Types

// region:    --- Public Functions

/// Serve the ACP protocol over stdio until the client disconnects.
///
/// `agent` selects the agent whose permissions overlay applies to every
/// session; `None` falls back to the configured default agent.
pub async fn serve(agent: Option<String>) -> Result<()> {
    // One ladder, one resolution: the config file the environment selects, then
    // every application path derived from it. The load below reads the file the
    // resolution names, so this entry point never re-derives a base.
    let loaded = toml_config::load();
    let paths = agnt_runtime::config::Paths::resolve_for_process(
        loaded
            .as_ref()
            .ok()
            .and_then(|config| config.paths.as_ref()),
    )
    .map_err(|err| AcpError::Config(format!("failed to resolve application paths: {err}")))?;
    agnt_runtime::logging::LogStore::retarget_process(&paths.log_dir);
    let grammar_stores =
        Arc::new(agnt_runtime::tools::handler::tree_sitter::GrammarStores::from_paths(&paths));
    let config_dir = paths.config_dir();
    let models_cache = match ModelsCache::load_from(&paths.models_cache_path) {
        Ok(cache) => Some(Arc::new(cache)),
        Err(err) => {
            log::debug!("models cache unavailable, advertising only the configured model: {err}");
            None
        }
    };
    let client_fs: Arc<StdMutex<Option<ClientFsState>>> = Arc::new(StdMutex::new(None));
    // Resolve the user hooks once: every session starts a runner for them on
    // its own bus. A config that fails to load or resolve leaves hooks empty,
    // so the server still serves. The same load supplies the model identity the
    // process-level agent hook envelopes carry.
    let (hooks, agent_hook_context) = match &loaded {
        Ok(config) => {
            let hooks = match agnt_runtime::config::resolve_hooks(config, config_dir) {
                Ok(hooks) => hooks,
                Err(err) => {
                    log::warn!("Failed to resolve hooks: {err}");
                    Vec::new()
                }
            };
            let context = HookAgentContext::default()
                .with_persona(persona_slug(
                    agent.as_deref(),
                    &paths.agents_dir,
                    &config.agents,
                ))
                .with_model(active_model_identity(config));
            (hooks, context)
        }
        Err(err) => {
            log::warn!("Failed to load config.toml for hooks: {err}");
            (Vec::new(), HookAgentContext::default())
        }
    };
    // The rig-memory store of each session is the store path the resolution
    // carries.
    let rig = agnt_runtime::session::RigStoreConfig::new(agnt_runtime::config::StoreType::Sqlite)
        .with_store_path(paths.store_path.clone());
    // Agent events are process-scoped, so they ride a standalone channel rather
    // than the per-session bus. The agent-class hook runner is spawned once for
    // this process; hooks with no agent-class event spawn nothing.
    let agent_tx = AgentTx::new("agent", 16);
    let agent_hook_handles = agnt_runtime::hooks::spawn_agent_hook_runner(
        &agent_tx,
        &hooks,
        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        &agent_hook_context,
        agnt_runtime::hooks::NuProcess,
    );
    let sessions: SharedSessions = Arc::new(
        AcpSessionManager::new(rig, paths, tokio::runtime::Handle::current())
            .with_grammar_stores(grammar_stores)
            .with_models_cache(models_cache)
            .with_client_fs(Arc::clone(&client_fs))
            .with_agent_name(agent)
            .with_hooks(hooks),
    );
    let _ = agent_tx
        .send(AgentEvent::Started {
            mode: "acp".to_string(),
        })
        .await;

    let result = Agent
        .builder()
        .name("nu-agent")
        .on_receive_request(
            {
                let client_fs = Arc::clone(&client_fs);
                async move |request: InitializeRequest, responder, connection| {
                    *lock_client_fs(&client_fs) = Some(ClientFsState {
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
                        Ok(session_id) => match sessions.config_options(&session_id).await {
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
                    let result = sessions
                        .close_session(&request.session_id.to_string())
                        .await;
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
        .await;

    // Best-effort shutdown: the pair is emitted on normal exit only. No
    // process-wide signal handler exists, so SIGTERM/SIGKILL produce no
    // emission. The drain shares one deadline across the runners, so a slow
    // hook never delays exit past it.
    let reason = if result.is_ok() { "shutdown" } else { "error" };
    let _ = agent_tx
        .send(AgentEvent::Shutdown {
            reason: reason.to_string(),
        })
        .await;
    drop(agent_tx);
    let deadline = tokio::time::Instant::now() + agnt_runtime::hooks::HOOK_DRAIN_TIMEOUT;
    for handle in agent_hook_handles {
        let _ = tokio::time::timeout_at(deadline, handle).await;
    }
    result?;

    Ok(())
}

// endregion: --- Public Functions

// region:    --- Support

/// The client's filesystem state, recovered if its writer panicked.
fn lock_client_fs(
    client_fs: &Arc<StdMutex<Option<ClientFsState>>>,
) -> std::sync::MutexGuard<'_, Option<ClientFsState>> {
    client_fs.lock().unwrap_or_else(|e| e.into_inner())
}

/// The persona slug `agent` names, or `None` when it names no persona.
///
/// The process-level agent hook envelopes carry this slug: agent events are
/// process-scoped, so they cannot read a session's snapshot.
fn persona_slug(
    agent: Option<&str>,
    agents_dir: &std::path::Path,
    agents_config: &AgentsConfig,
) -> Option<String> {
    let name = agent?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let resolver = FsPersonaResolver::new(cwd, agents_dir.to_path_buf(), agents_config.clone());
    resolver.resolve(name).ok().map(|_| name.to_string())
}

/// The `provider/model` identity of the config's default role.
///
/// Returns an empty string when the role does not resolve: the server cannot
/// serve a session then either, and a hook envelope must never fail a run.
fn active_model_identity(plugin_config: &PluginConfig) -> String {
    let env = agnt_runtime::utils::env_map::process_env();
    match agnt_runtime::config::resolve_role_config(plugin_config, None, &env) {
        Ok(config) => agnt_runtime::protocol::picker::format_active_model_identity(
            &config.provider,
            &config.model,
        ),
        Err(err) => {
            log::warn!("Failed to resolve the active model for hook envelopes: {err}");
            String::new()
        }
    }
}

// endregion: --- Support
