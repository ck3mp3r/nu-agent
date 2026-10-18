//! Shared support for the ACP stdio integration tests.
//!
//! Every test builds a [`TestEnv`] — a throwaway XDG config/cache tree plus a
//! working directory — and a wiremock HTTP server that answers the agent's
//! Ollama completion requests with canned SSE. The agent itself runs as a
//! real `agent acp` subprocess launched through the ACP SDK, so the
//! tests exercise the stdio transport end to end.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    ContentBlock, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason,
};
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, ActiveSession, Agent, Builder, Client, HandleDispatchFrom, NullClose,
    NullRun, SessionMessage, on_receive_request,
};
use agnt_testkit::MockServer;

pub type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- TestEnv

/// A throwaway environment for one ACP subprocess.
///
/// The temp directory holds three trees: `config/` (the `XDG_CONFIG_HOME`
/// containing `nu-agent/config.toml`), `cache/` (the `XDG_CACHE_HOME` holding
/// the SQLite session store) and `cwd/` (the session working directory).
/// Dropping the value removes all three.
pub struct TestEnv {
    temp: tempfile::TempDir,
    config_home: PathBuf,
    cache_home: PathBuf,
    cwd: PathBuf,
    binary: PathBuf,
}

impl TestEnv {
    /// Create the environment and write a config.toml that points the `default`
    /// model role at `llm_base_url` through the `ollama` provider.
    ///
    /// `extra_config` is appended verbatim, so a test can add a `[permissions]`
    /// block without duplicating the provider wiring.
    pub fn new(llm_base_url: &str, extra_config: &str) -> Result<Self> {
        let temp = tempfile::tempdir()?;
        let config_home = temp.path().join("config");
        let cache_home = temp.path().join("cache");
        let cwd = temp.path().join("cwd");
        std::fs::create_dir_all(config_home.join("nu-agent"))?;
        std::fs::create_dir_all(&cache_home)?;
        std::fs::create_dir_all(&cwd)?;

        let config = format!(
            "[models.default]\nmodel = \"ollama/llama3\"\n\n[providers.ollama]\nbase_url = \"{llm_base_url}\"\n\n{extra_config}\n"
        );
        std::fs::write(config_home.join("nu-agent").join("config.toml"), config)?;

        Ok(Self {
            temp,
            config_home,
            cache_home,
            cwd,
            binary: PathBuf::from(env!("CARGO_BIN_EXE_agent")),
        })
    }

    /// The session working directory handed to `session/new`.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// An `AcpAgent` that launches `agent acp` with this environment.
    ///
    /// `AGENT_VAULT_BACKEND=env` keeps the subprocess config load from probing
    /// the OS keychain.
    pub fn agent(&self) -> AcpAgent {
        AcpAgent::new(
            AcpAgentConfig::new(&self.binary)
                .arg("acp")
                .env("AGENT_VAULT_BACKEND", "env")
                .env(
                    "XDG_CONFIG_HOME",
                    self.config_home.to_string_lossy().into_owned(),
                )
                .env(
                    "XDG_CACHE_HOME",
                    self.cache_home.to_string_lossy().into_owned(),
                )
                .env("HOME", self.temp.path().to_string_lossy().into_owned()),
        )
    }
}

// endregion: --- TestEnv

// region:    --- Mock LLM

/// Start a wiremock server that stands in for the Ollama daemon.
pub async fn start_llm_server() -> Result<MockServer> {
    Ok(agnt_testkit::start_server().await)
}

/// Answer every completion request with `text`, optionally after `delay`.
pub async fn mount_text_response(
    server: &MockServer,
    text: &str,
    delay: Option<Duration>,
) -> Result<()> {
    agnt_testkit::mount_text_response(server, text, delay).await;
    Ok(())
}

/// Answer the first completion request with a tool call and every later one
/// with `final_text`.
///
/// The agent runs one completion per tool round-trip, so the first request is
/// the one that must produce the call and the second is the one that sees the
/// tool result.
pub async fn mount_tool_then_text(
    server: &MockServer,
    tool_name: &str,
    arguments: serde_json::Value,
    final_text: &str,
) -> Result<()> {
    agnt_testkit::mount_tool_then_text(server, tool_name, arguments, final_text).await;
    Ok(())
}

/// The number of completion requests the mock server has received so far.
pub async fn llm_request_count(server: &MockServer) -> usize {
    agnt_testkit::received_request_count(server).await
}

/// The raw bodies of every completion request the mock server has received.
pub async fn llm_request_bodies(server: &MockServer) -> Vec<String> {
    agnt_testkit::received_request_bodies(server).await
}

// endregion: --- Mock LLM

// region:    --- Client harness

/// Records the permission requests the agent sends to the client.
#[derive(Default)]
pub struct Recorder {
    permission_requests: Mutex<Vec<RequestPermissionRequest>>,
}

impl Recorder {
    /// The permission requests the agent sent, in arrival order.
    pub fn permission_requests(&self) -> Vec<RequestPermissionRequest> {
        self.permission_requests
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

/// Build a client that answers every permission request with the first option
/// (`allow-once`) and records the request.
pub fn build_client(
    recorder: Arc<Recorder>,
) -> Builder<Client, impl HandleDispatchFrom<Agent>, NullRun, NullClose> {
    Client
        .builder()
        .name("agnt-acp-test-client")
        .on_receive_request(
            {
                let recorder = Arc::clone(&recorder);
                async move |request: RequestPermissionRequest, responder, _cx| {
                    let outcome = request
                        .options
                        .first()
                        .map(|option| {
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option.option_id.clone(),
                            ))
                        })
                        .unwrap_or(RequestPermissionOutcome::Cancelled);
                    if let Ok(mut guard) = recorder.permission_requests.lock() {
                        guard.push(request);
                    }
                    responder.respond(RequestPermissionResponse::new(outcome))
                }
            },
            on_receive_request!(),
        )
}

/// Wrap an error as an ACP internal error so it can cross the client closure.
pub fn acp_err(message: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(message.to_string())
}

// endregion: --- Client harness

// region:    --- Turn helpers

/// How long a single turn may take before the test fails instead of hanging.
const TURN_TIMEOUT: Duration = Duration::from_secs(30);

/// Read one turn's updates and return the agent text plus the stop reason.
///
/// Fails rather than hangs when the turn never reports a stop reason, so a
/// broken handler surfaces as a test failure.
pub async fn read_turn(
    session: &mut ActiveSession<'static, Agent>,
) -> Result<(String, StopReason)> {
    tokio::time::timeout(TURN_TIMEOUT, read_turn_inner(session))
        .await
        .map_err(|_| "turn did not report a stop reason within the timeout")?
}

async fn read_turn_inner(
    session: &mut ActiveSession<'static, Agent>,
) -> Result<(String, StopReason)> {
    let mut text = String::new();
    loop {
        match session.read_update().await? {
            SessionMessage::SessionMessage(dispatch) => {
                MatchDispatch::new(dispatch)
                    .if_notification(async |notification: SessionNotification| {
                        if let SessionUpdate::AgentMessageChunk(chunk) = notification.update
                            && let ContentBlock::Text(content) = chunk.content
                        {
                            text.push_str(&content.text);
                        }
                        Ok(())
                    })
                    .await
                    .otherwise_ignore()?;
            }
            SessionMessage::StopReason(reason) => return Ok((text, reason)),
            _ => {}
        }
    }
}

/// Collect the replayed message chunks of a restored session until `window`
/// elapses with no further update.
///
/// `session/load` replays the stored conversation as `session/update`
/// notifications and has no end marker, so a quiet window is the only signal
/// that the replay is complete.
pub async fn collect_replay(
    session: &mut ActiveSession<'static, Agent>,
    window: Duration,
) -> Result<Vec<String>> {
    let mut chunks = Vec::new();
    loop {
        match tokio::time::timeout(window, session.read_update()).await {
            Ok(Ok(SessionMessage::SessionMessage(dispatch))) => {
                MatchDispatch::new(dispatch)
                    .if_notification(async |notification: SessionNotification| {
                        match notification.update {
                            SessionUpdate::UserMessageChunk(chunk)
                            | SessionUpdate::AgentMessageChunk(chunk) => {
                                if let ContentBlock::Text(content) = chunk.content {
                                    chunks.push(content.text);
                                }
                            }
                            _ => {}
                        }
                        Ok(())
                    })
                    .await
                    .otherwise_ignore()?;
            }
            Ok(Ok(SessionMessage::StopReason(_))) => break,
            Ok(Err(err)) => return Err(err.into()),
            Err(_elapsed) => break,
            _ => {}
        }
    }
    Ok(chunks)
}

// endregion: --- Turn helpers
