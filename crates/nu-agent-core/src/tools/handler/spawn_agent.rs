use std::collections::HashMap;
use std::path::PathBuf;

use crate::tools::handler::ToolHandlerError;

/// Orchestrator state for multi-agent spawning
pub struct OrchestratorState {
    pub socket_dir: PathBuf,
    pub spawn_count: usize,
    pub tmux_window: Option<String>,
    pub cwd: PathBuf,
    pub agent_identity: Option<String>,
    pub agent_panes: HashMap<String, String>,
}

impl OrchestratorState {
    pub fn new(cwd: PathBuf) -> Self {
        let socket_dir = crate::mailbox::socket_dir_for_path(&cwd);
        Self {
            socket_dir,
            spawn_count: 0,
            tmux_window: None,
            cwd,
            agent_identity: None,
            agent_panes: HashMap::new(),
        }
    }
}

/// Trait for tmux command execution (enables testing with mocks)
pub trait TmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolHandlerError>;
}

/// Real tmux runner using std::process::Command
pub struct RealTmuxRunner;

impl TmuxRunner for RealTmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolHandlerError> {
        let output = std::process::Command::new("tmux")
            .args(args)
            .output()
            .map_err(|e| ToolHandlerError::runtime(format!("Failed to execute tmux: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolHandlerError::runtime(format!(
                "tmux command failed: {}",
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Shell escape a string for safe command construction
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Poll until the shell in the given pane is ready to accept commands
async fn wait_for_shell_ready<T: TmuxRunner>(
    tmux: &T,
    pane_id: &str,
    timeout: std::time::Duration,
) -> Result<(), ToolHandlerError> {
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
            return Err(ToolHandlerError::runtime(format!(
                "Shell not ready after {}s (current: '{}')",
                timeout.as_secs(),
                cmd
            )));
        }
        tokio::time::sleep(poll_interval).await;
    }
}

#[cfg(test)]
pub async fn wait_for_shell_ready_pub<T: TmuxRunner>(
    tmux: &T,
    pane_id: &str,
    timeout: std::time::Duration,
) -> Result<(), ToolHandlerError> {
    wait_for_shell_ready(tmux, pane_id, timeout).await
}

/// Intermediate result of the synchronous phase of spawning an agent.
/// Holds everything needed by `finish_spawn_agent` to complete the operation.
#[derive(Debug)]
pub struct SpawnPrepared {
    pub assigned_name: String,
    pub pane_id: String,
    pub cmd: String,
}

/// Synchronous phase: parse args, assign name, create tmux pane, build the command string.
/// Does NOT call `wait_for_shell_ready` so the caller can drop any MutexGuard before awaiting.
pub fn prepare_spawn_agent<T: TmuxRunner>(
    args: &serde_json::Value,
    state: &mut OrchestratorState,
    tmux: &T,
) -> Result<SpawnPrepared, ToolHandlerError> {
    // 1. Parse arguments
    let agent = args["agent"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::validation("Missing required 'agent' parameter"))?;
    let name = args.get("name").and_then(|v| v.as_str());

    // 2. Assign name (auto-generate if not provided)
    state.spawn_count += 1;
    let assigned_name = name
        .map(String::from)
        .unwrap_or_else(|| format!("{}-{}", agent, state.spawn_count));

    // 2a. Check for duplicate name before any tmux work
    if state.agent_panes.contains_key(&assigned_name) {
        state.spawn_count -= 1; // undo the increment
        return Err(ToolHandlerError::runtime(format!(
            "Agent named '{}' already exists. Terminate it first or choose a different name.",
            assigned_name
        )));
    }

    log::debug!("Spawning agent: {} (persona: {})", assigned_name, agent);

    // 3. Tmux pane management — with stale window recovery
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

    let pane_id =
        pane_id.ok_or_else(|| ToolHandlerError::runtime("Failed to obtain tmux pane ID"))?;

    // 4a. Track agent pane for terminate_agent
    state
        .agent_panes
        .insert(assigned_name.clone(), pane_id.clone());

    // 4b. Re-tile panes evenly
    let window_ref = state
        .tmux_window
        .as_ref()
        .ok_or_else(|| ToolHandlerError::runtime("tmux window not initialized"))?;
    tmux.run(&["select-layout", "-t", window_ref, "even-horizontal"])?;

    // 5. Build command string
    let parent_name = state.agent_identity.as_deref().unwrap_or("orchestrator");
    let cmd = format!(
        "agent --agent {} --name {} --parent-name {}",
        shell_escape(agent),
        shell_escape(&assigned_name),
        shell_escape(parent_name)
    );

    Ok(SpawnPrepared {
        assigned_name,
        pane_id,
        cmd,
    })
}

/// Async phase: wait for shell to be ready, then send the launch command.
/// Caller must ensure no MutexGuard is held when calling this.
pub async fn finish_spawn_agent<T: TmuxRunner>(
    prepared: SpawnPrepared,
    tmux: &T,
) -> Result<serde_json::Value, ToolHandlerError> {
    // 4c. Wait for the shell to be ready in the new pane
    wait_for_shell_ready(tmux, &prepared.pane_id, std::time::Duration::from_secs(30)).await?;

    // 5. Send command to pane
    log::debug!("Sending command to tmux pane: {}", prepared.cmd);
    tmux.run(&["send-keys", "-t", &prepared.pane_id, &prepared.cmd, "Enter"])?;

    // 6. Return result
    Ok(serde_json::json!({
        "name": prepared.assigned_name
    }))
}

/// Handle spawn_agent tool invocation
pub async fn handle_spawn_agent<T: TmuxRunner>(
    args: &serde_json::Value,
    state: &mut OrchestratorState,
    tmux: &T,
) -> Result<serde_json::Value, ToolHandlerError> {
    let prepared = prepare_spawn_agent(args, state, tmux)?;
    finish_spawn_agent(prepared, tmux).await
}

/// Dispatch builtin spawn_agent tool
pub async fn dispatch_spawn_agent(
    arguments: &serde_json::Value,
    state: &mut OrchestratorState,
) -> Result<Option<serde_json::Value>, ToolHandlerError> {
    let tmux = RealTmuxRunner;
    handle_spawn_agent(arguments, state, &tmux).await.map(Some)
}

/// Handle terminate_agent tool invocation
pub fn handle_terminate_agent<T: TmuxRunner>(
    args: &serde_json::Value,
    state: &mut OrchestratorState,
    tmux: &T,
) -> Result<serde_json::Value, ToolHandlerError> {
    // 1. Extract name
    let name = args["name"]
        .as_str()
        .ok_or_else(|| ToolHandlerError::validation("Missing required 'name' parameter"))?;

    // 2. Look up pane_id
    let pane_id = state.agent_panes.get(name).cloned().ok_or_else(|| {
        ToolHandlerError::runtime(format!("No agent named '{}' is running", name))
    })?;

    // 3. Kill the pane only if still alive (ignore errors — pane may already be dead)
    if is_pane_alive(tmux, &pane_id) {
        let _ = tmux.run(&["kill-pane", "-t", &pane_id]);
    }

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
pub fn dispatch_terminate_agent(
    arguments: &serde_json::Value,
    state: &mut OrchestratorState,
) -> Result<Option<serde_json::Value>, ToolHandlerError> {
    let tmux = RealTmuxRunner;
    handle_terminate_agent(arguments, state, &tmux).map(Some)
}

/// Check whether a tmux pane is still alive.
/// Returns false if tmux is unavailable, the pane doesn't exist, or any error occurs.
pub(crate) fn is_pane_alive<T: TmuxRunner>(tmux: &T, pane_id: &str) -> bool {
    match tmux.run(&["list-panes", "-a", "-F", "#{pane_id}"]) {
        Ok(output) => output.lines().any(|line| line.trim() == pane_id),
        Err(_) => false,
    }
}
