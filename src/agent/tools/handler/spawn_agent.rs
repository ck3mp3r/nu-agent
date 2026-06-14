#![allow(private_interfaces)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agent::mailbox::{AgentRegistry, Broker};

/// Error type for spawn_agent tool execution
#[derive(Debug, Clone)]
pub(crate) struct ToolExecError {
    pub(crate) message: String,
    pub(crate) details: Option<serde_json::Value>,
}

impl ToolExecError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }

    fn execution(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }
}

/// Orchestrator state for multi-agent spawning
pub struct OrchestratorState {
    pub(crate) broker: Option<Broker>,
    pub(crate) registry: Arc<RwLock<AgentRegistry>>,
    pub(crate) socket_path: Option<PathBuf>,
    pub(crate) spawn_count: usize,
    pub(crate) tmux_window: Option<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) agent_identity: Option<String>,
    pub(crate) agent_panes: HashMap<String, String>,
}

impl OrchestratorState {
    pub fn new(registry: Arc<RwLock<AgentRegistry>>, cwd: PathBuf) -> Self {
        Self {
            broker: None,
            registry,
            socket_path: None,
            spawn_count: 0,
            tmux_window: None,
            cwd,
            agent_identity: None,
            agent_panes: HashMap::new(),
        }
    }
}

/// Trait for tmux command execution (enables testing with mocks)
pub(crate) trait TmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolExecError>;
}

/// Real tmux runner using std::process::Command
pub(crate) struct RealTmuxRunner;

impl TmuxRunner for RealTmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolExecError> {
        let output = std::process::Command::new("tmux")
            .args(args)
            .output()
            .map_err(|e| ToolExecError::execution(format!("Failed to execute tmux: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolExecError::execution(format!(
                "tmux command failed: {}",
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Generate a random hex token
pub(crate) fn generate_hex_token(bytes: usize) -> String {
    let mut random_bytes = vec![0u8; bytes];
    rand::fill(&mut random_bytes[..]);
    random_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

/// Shell escape a string for safe command construction
fn shell_escape(s: &str) -> String {
    // Simple shell escaping: wrap in single quotes and escape single quotes
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Poll until the shell in the given pane is ready to accept commands
fn wait_for_shell_ready<T: TmuxRunner>(
    tmux: &T,
    pane_id: &str,
    timeout: std::time::Duration,
) -> Result<(), ToolExecError> {
    let known_shells = ["bash", "zsh", "nu", "fish", "sh", "dash"];
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(200);
    loop {
        let output = tmux.run(&[
            "display-message",
            "-p",
            "-t",
            pane_id,
            "#{pane_current_command}",
        ])?;
        let cmd = output.trim();
        if known_shells.contains(&cmd) {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(ToolExecError::execution(format!(
                "Shell not ready after {}s (current: '{}')",
                timeout.as_secs(),
                cmd
            )));
        }
        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
pub(crate) fn wait_for_shell_ready_pub<T: TmuxRunner>(
    tmux: &T,
    pane_id: &str,
    timeout: std::time::Duration,
) -> Result<(), ToolExecError> {
    wait_for_shell_ready(tmux, pane_id, timeout)
}

/// Handle spawn_agent tool invocation
pub(crate) fn handle_spawn_agent<T: TmuxRunner>(
    args: &serde_json::Value,
    state: &mut OrchestratorState,
    tmux: &T,
) -> Result<serde_json::Value, ToolExecError> {
    // 1. Parse arguments
    let agent = args["agent"]
        .as_str()
        .ok_or_else(|| ToolExecError::validation("Missing required 'agent' parameter"))?;
    let name = args.get("name").and_then(|v| v.as_str());

    // 2. Lazy broker initialization
    if state.broker.is_none() {
        log::debug!("Initializing broker for first spawn");
        let broker = Broker::start(Arc::clone(&state.registry))
            .map_err(|e| ToolExecError::execution(format!("Failed to start broker: {}", e)))?;
        state.socket_path = Some(broker.socket_path().to_path_buf());
        state.broker = Some(broker);
    }

    // 3. Assign name (auto-generate if not provided)
    state.spawn_count += 1;
    let assigned_name = name
        .map(String::from)
        .unwrap_or_else(|| format!("{}-{}", agent, state.spawn_count));

    log::debug!("Spawning agent: {} (persona: {})", assigned_name, agent);

    // 4. Generate token and register
    let token = generate_hex_token(32);
    state
        .registry
        .try_write()
        .map_err(|_| ToolExecError::execution("Failed to acquire registry lock"))?
        .register_pending(token.clone(), assigned_name.clone());

    // 5. Tmux pane management — with stale window recovery
    let socket_path = state.socket_path.as_ref().unwrap();
    let cwd_str = state.cwd.display().to_string();

    // Try splitting existing window; on failure, clear stale reference
    let mut pane_id: Option<String> = None;
    if let Some(window) = state.tmux_window.take() {
        log::debug!("Splitting existing tmux window: {}", window);
        match tmux.run(&[
            "split-window",
            "-h",
            "-c",
            &cwd_str,
            "-P",
            "-F",
            "#{pane_id}",
            "-t",
            &window,
        ]) {
            Ok(id) => {
                pane_id = Some(id.trim().to_string());
                state.tmux_window = Some(window);
            }
            Err(e) => {
                log::warn!(
                    "split-window failed (stale window?), will create new: {}",
                    e.message
                );
                // tmux_window already None from take()
            }
        }
    }

    // Discover existing "agents" window if we don't have one
    if state.tmux_window.is_none()
        && let Ok(output) = tmux.run(&["list-windows", "-F", "#{window_id}\t#{window_name}"])
    {
        for line in output.lines() {
            let mut parts = line.splitn(2, '\t');
            if let (Some(wid), Some(wname)) = (parts.next(), parts.next())
                && wname.trim() == "agents"
            {
                log::debug!("Discovered existing 'agents' tmux window: {}", wid);
                let window_id = wid.trim().to_string();
                match tmux.run(&[
                    "split-window",
                    "-h",
                    "-c",
                    &cwd_str,
                    "-P",
                    "-F",
                    "#{pane_id}",
                    "-t",
                    &window_id,
                ]) {
                    Ok(id) => {
                        pane_id = Some(id.trim().to_string());
                        state.tmux_window = Some(window_id);
                    }
                    Err(e) => {
                        log::warn!("split-window into discovered window failed: {}", e.message);
                    }
                }
                break;
            }
        }
    }

    // Create new window if needed (either originally None or after failed split)
    if state.tmux_window.is_none() {
        log::debug!("Creating new tmux window for agents");
        let output = tmux.run(&[
            "new-window",
            "-c",
            &cwd_str,
            "-P",
            "-F",
            "#{window_id}\t#{pane_id}",
            "-n",
            "agents",
        ])?;
        let mut parts = output.trim().splitn(2, '\t');
        let window_id = parts.next().unwrap_or("").to_string();
        let pid = parts.next().unwrap_or("").to_string();
        state.tmux_window = Some(window_id);
        pane_id = Some(pid);
    }

    let pane_id = pane_id.unwrap();

    // 5a. Track agent pane for terminate_agent
    state
        .agent_panes
        .insert(assigned_name.clone(), pane_id.clone());

    // 5b. Re-tile panes evenly
    let window_ref = state.tmux_window.as_ref().unwrap();
    tmux.run(&["select-layout", "-t", window_ref, "even-horizontal"])?;

    // 5c. Wait for the shell to be ready in the new pane
    wait_for_shell_ready(tmux, &pane_id, std::time::Duration::from_secs(30))?;

    // 6. Send command to pane
    let parent_name = state.agent_identity.as_deref().unwrap_or("orchestrator");
    let cmd = format!(
        "agent --agent {} --name {} --broker-socket {} --broker-token {} --parent-name {}",
        shell_escape(agent),
        shell_escape(&assigned_name),
        shell_escape(&socket_path.display().to_string()),
        &token,
        shell_escape(parent_name)
    );
    log::debug!("Sending command to tmux pane: {}", cmd);
    tmux.run(&["send-keys", "-t", &pane_id, &cmd, "Enter"])?;

    // 7. Return result
    Ok(serde_json::json!({
        "name": assigned_name
    }))
}

/// Dispatch builtin spawn_agent tool
pub(crate) fn dispatch_spawn_agent(
    arguments: &serde_json::Value,
    state: &mut OrchestratorState,
) -> Result<Option<serde_json::Value>, ToolExecError> {
    let tmux = RealTmuxRunner;
    handle_spawn_agent(arguments, state, &tmux).map(Some)
}

/// Handle terminate_agent tool invocation
pub(crate) fn handle_terminate_agent<T: TmuxRunner>(
    args: &serde_json::Value,
    state: &mut OrchestratorState,
    tmux: &T,
) -> Result<serde_json::Value, ToolExecError> {
    // 1. Extract name
    let name = args["name"]
        .as_str()
        .ok_or_else(|| ToolExecError::validation("Missing required 'name' parameter"))?;

    // 2. Look up pane_id
    let pane_id =
        state.agent_panes.get(name).cloned().ok_or_else(|| {
            ToolExecError::execution(format!("No agent named '{}' is running", name))
        })?;

    // 3. Kill the pane (ignore errors — pane may already be dead)
    let _ = tmux.run(&["kill-pane", "-t", &pane_id]);

    // 4. Remove from agent_panes
    state.agent_panes.remove(name);

    // 5. Check if tmux window still has panes
    if let Some(ref window_id) = state.tmux_window {
        let remaining = tmux
            .run(&["list-panes", "-t", window_id, "-F", "#{pane_id}"])
            .unwrap_or_default();
        if remaining.trim().is_empty() {
            state.tmux_window = None;
        }
    }

    // 6. Return result
    Ok(serde_json::json!({ "terminated": name }))
}

/// Dispatch builtin terminate_agent tool
pub(crate) fn dispatch_terminate_agent(
    arguments: &serde_json::Value,
    state: &mut OrchestratorState,
) -> Result<Option<serde_json::Value>, ToolExecError> {
    let tmux = RealTmuxRunner;
    handle_terminate_agent(arguments, state, &tmux).map(Some)
}
