use std::sync::Arc;
use tempfile::TempDir;

use super::ToolHandlerError;
use super::spawn_agent::{
    OrchestratorState, TmuxRunner, handle_spawn_agent, handle_terminate_agent,
};

/// Mock TmuxRunner for testing.
struct MockTmuxRunner {
    calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    window_response: String,
    pane_response: String,
    display_responses: Vec<String>,
    display_call_count: Arc<std::sync::Mutex<usize>>,
    list_panes_response: String,
    list_windows_response: Option<String>,
}

impl MockTmuxRunner {
    fn new(pane_id: impl Into<String>) -> Self {
        let pane = pane_id.into();
        Self {
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            window_response: "@1".to_string(),
            pane_response: pane,
            display_responses: vec!["nu\n".to_string()],
            display_call_count: Arc::new(std::sync::Mutex::new(0)),
            list_panes_response: String::new(),
            list_windows_response: None,
        }
    }

    fn with_window(mut self, window_id: impl Into<String>) -> Self {
        self.window_response = window_id.into();
        self
    }

    fn with_display_responses(mut self, responses: Vec<String>) -> Self {
        self.display_responses = responses;
        self
    }

    fn with_list_panes_response(mut self, response: impl Into<String>) -> Self {
        self.list_panes_response = response.into();
        self
    }

    fn with_list_windows_response(mut self, response: &str) -> Self {
        self.list_windows_response = Some(response.to_string());
        self
    }

    fn get_calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockTmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolHandlerError> {
        let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.calls.lock().unwrap().push(args_owned.clone());
        if args_owned.contains(&"display-message".to_string()) {
            let mut count = self.display_call_count.lock().unwrap();
            let idx = (*count).min(self.display_responses.len().saturating_sub(1));
            *count += 1;
            return Ok(self.display_responses[idx].clone());
        }
        if args_owned.contains(&"new-window".to_string()) {
            return Ok(format!(
                "{}\t{}\n",
                self.window_response, self.pane_response
            ));
        }
        if args_owned.contains(&"split-window".to_string()) {
            return Ok(format!("{}\n", self.pane_response));
        }
        if args_owned.contains(&"kill-pane".to_string()) {
            return Ok(String::new());
        }
        if args_owned.contains(&"list-panes".to_string()) {
            return Ok(self.list_panes_response.clone());
        }
        if args_owned.contains(&"list-windows".to_string()) {
            return Ok(self.list_windows_response.clone().unwrap_or_default());
        }
        Ok(String::new())
    }
}

// ============================================================================
// spawn_agent tests
// ============================================================================

#[tokio::test]
async fn handle_spawn_agent_increments_spawn_count() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(result.is_ok(), "First spawn should succeed");
    assert_eq!(state.spawn_count, 1, "Spawn count should be incremented");
}

#[tokio::test]
async fn handle_spawn_agent_auto_names() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let args1 = serde_json::json!({ "agent": "researcher" });
    let result1 = handle_spawn_agent(&args1, &mut state, &tmux).expect("First spawn failed");
    assert_eq!(result1["name"].as_str().unwrap(), "researcher-1");

    let args2 = serde_json::json!({ "agent": "researcher" });
    let result2 = handle_spawn_agent(&args2, &mut state, &tmux).expect("Second spawn failed");
    assert_eq!(result2["name"].as_str().unwrap(), "researcher-2");
}

#[tokio::test]
async fn handle_spawn_agent_explicit_name() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let args = serde_json::json!({ "agent": "researcher", "name": "my-researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");
    assert_eq!(result["name"].as_str().unwrap(), "my-researcher");
}

#[tokio::test]
async fn handle_spawn_agent_creates_tmux_window() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let args = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls = tmux.get_calls();
    assert!(!calls.is_empty(), "Tmux should have been called");

    assert!(
        calls[0].contains(&"list-windows".to_string()),
        "First call should be list-windows for discovery"
    );
    assert!(
        calls[1].contains(&"new-window".to_string()),
        "Second call should create a new window"
    );
    assert!(
        calls[2].contains(&"select-layout".to_string())
            && calls[2].contains(&"even-horizontal".to_string()),
        "Third call should select-layout even-horizontal"
    );
}

#[tokio::test]
async fn handle_spawn_agent_splits_pane_on_second_spawn() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    )
    .expect("First spawn failed");
    handle_spawn_agent(&serde_json::json!({ "agent": "coder" }), &mut state, &tmux)
        .expect("Second spawn failed");

    let calls = tmux.get_calls();
    let split_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"split-window".to_string()))
        .collect();
    assert!(
        !split_calls.is_empty(),
        "Second spawn should split the window"
    );
    assert!(
        split_calls[0].contains(&"-h".to_string()),
        "split-window should use -h flag"
    );

    let layout_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"select-layout".to_string()))
        .collect();
    assert!(
        !layout_calls.is_empty(),
        "Should call select-layout after split"
    );
    assert!(layout_calls[0].contains(&"even-horizontal".to_string()));
}

#[tokio::test]
async fn handle_spawn_agent_sends_command() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let args = serde_json::json!({ "agent": "researcher", "name": "test-agent" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls = tmux.get_calls();
    let send_keys: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"send-keys".to_string()))
        .collect();
    assert!(!send_keys.is_empty(), "Should send command to tmux pane");

    let command = send_keys[0].join(" ");
    assert!(command.contains("--agent") && command.contains("'researcher'"));
    assert!(command.contains("--name") && command.contains("'test-agent'"));
    assert!(
        !command.contains("--broker-socket"),
        "Command must NOT contain --broker-socket"
    );
    assert!(
        !command.contains("--broker-token"),
        "Command must NOT contain --broker-token"
    );
}

#[tokio::test]
async fn handle_spawn_agent_includes_parent_name_from_identity() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());
    state.agent_identity = Some("my-orchestrator".to_string());

    let args = serde_json::json!({ "agent": "researcher", "name": "test-agent" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls = tmux.get_calls();
    let send_keys: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"send-keys".to_string()))
        .collect();
    let command = send_keys[0].join(" ");
    assert!(
        command.contains("--parent-name") && command.contains("'my-orchestrator'"),
        "Command should contain --parent-name with orchestrator identity, got: {command}"
    );
}

#[tokio::test]
async fn handle_spawn_agent_defaults_parent_name_to_orchestrator() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("@1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let args = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls = tmux.get_calls();
    let send_keys: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"send-keys".to_string()))
        .collect();
    let command = send_keys[0].join(" ");
    assert!(
        command.contains("--parent-name") && command.contains("'orchestrator'"),
        "Command should default --parent-name to 'orchestrator', got: {command}"
    );
}

/// Mock TmuxRunner that fails on split-window but succeeds on new-window
struct StaleTmuxRunner {
    calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    new_window_id: String,
}

impl StaleTmuxRunner {
    fn new(new_window_id: String) -> Self {
        Self {
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            new_window_id,
        }
    }

    fn get_calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for StaleTmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolHandlerError> {
        let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.calls.lock().unwrap().push(args_owned.clone());
        if args_owned.contains(&"split-window".to_string()) {
            return Err(ToolHandlerError::runtime("can't find window: @dead"));
        }
        if args_owned.contains(&"new-window".to_string()) {
            return Ok(format!("{}\t%2\n", self.new_window_id));
        }
        if args_owned.contains(&"display-message".to_string()) {
            return Ok("nu\n".to_string());
        }
        Ok(String::new())
    }
}

#[tokio::test]
async fn handle_spawn_agent_recovers_from_stale_tmux_window() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = StaleTmuxRunner::new("@2".to_string());
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    // Simulate stale window reference
    state.tmux_window = Some("@dead".to_string());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(
        result.is_ok(),
        "Spawn should recover from stale window: {result:?}"
    );
    assert_eq!(state.tmux_window.as_deref(), Some("@2"));

    let calls = tmux.get_calls();
    let cmd_sequence: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
    assert_eq!(
        cmd_sequence,
        vec![
            "split-window",
            "list-windows",
            "new-window",
            "select-layout",
            "display-message",
            "send-keys"
        ],
        "Should attempt split, discover via list-windows, fallback to new-window"
    );
}

#[tokio::test]
async fn shell_ready_immediately() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let result = handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    );
    assert!(
        result.is_ok(),
        "Should succeed when shell ready immediately"
    );

    let calls = tmux.get_calls();
    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert!(!send_keys.is_empty(), "send-keys should be called");
}

#[tokio::test]
async fn shell_ready_after_delay() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%2").with_display_responses(vec![
        "direnv\n".to_string(),
        "direnv\n".to_string(),
        "nu\n".to_string(),
    ]);
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let result = handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    );
    assert!(result.is_ok(), "Should succeed after delayed readiness");

    let calls = tmux.get_calls();
    let display_calls: Vec<_> = calls.iter().filter(|c| c[0] == "display-message").collect();
    assert_eq!(
        display_calls.len(),
        3,
        "Should have polled 3 times before ready"
    );
}

#[tokio::test]
async fn shell_never_ready_timeout() {
    struct NeverReadyRunner {
        calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    }
    impl TmuxRunner for NeverReadyRunner {
        fn run(&self, args: &[&str]) -> Result<String, ToolHandlerError> {
            let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            self.calls.lock().unwrap().push(args_owned.clone());
            if args_owned.contains(&"display-message".to_string()) {
                return Ok("direnv\n".to_string());
            }
            if args_owned.contains(&"new-window".to_string()) {
                return Ok("%3\n".to_string());
            }
            Ok(String::new())
        }
    }

    let tmux = NeverReadyRunner {
        calls: Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    let result = super::spawn_agent::wait_for_shell_ready_pub(
        &tmux,
        "%3",
        std::time::Duration::from_millis(100),
    );
    assert!(result.is_err(), "Should timeout when shell never ready");
    assert!(result.unwrap_err().message.contains("Shell not ready"));

    let calls = tmux.calls.lock().unwrap().clone();
    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert!(
        send_keys.is_empty(),
        "send-keys should not be called on timeout"
    );
}

#[tokio::test]
async fn pane_id_captured_and_used() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%10").with_window("@10");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    )
    .expect("Spawn failed");

    let calls = tmux.get_calls();
    let new_window: Vec<_> = calls.iter().filter(|c| c[0] == "new-window").collect();
    assert!(new_window[0].contains(&"#{window_id}\t#{pane_id}".to_string()));

    let display: Vec<_> = calls.iter().filter(|c| c[0] == "display-message").collect();
    assert!(
        display[0].contains(&"%10".to_string()),
        "display-message should target %10"
    );

    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert!(
        send_keys[0].contains(&"%10".to_string()),
        "send-keys should target %10"
    );

    assert_eq!(state.tmux_window.as_deref(), Some("@10"));
}

#[tokio::test]
async fn split_window_captures_pane_id() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%20");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    )
    .expect("First spawn failed");
    handle_spawn_agent(&serde_json::json!({ "agent": "coder" }), &mut state, &tmux)
        .expect("Second spawn failed");

    let calls = tmux.get_calls();
    let splits: Vec<_> = calls.iter().filter(|c| c[0] == "split-window").collect();
    assert!(splits[0].contains(&"#{pane_id}".to_string()));

    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert_eq!(send_keys.len(), 2);
    assert!(send_keys[1].contains(&"%20".to_string()));
}

// ============================================================================
// terminate_agent tests
// ============================================================================

#[tokio::test]
async fn terminate_agent_kills_pane() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%5")
        .with_window("@5")
        .with_list_panes_response("%99\n");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher", "name": "r1" }),
        &mut state,
        &tmux,
    )
    .expect("spawn failed");

    let result = handle_terminate_agent(&serde_json::json!({ "name": "r1" }), &mut state, &tmux)
        .expect("terminate failed");
    assert_eq!(result["terminated"].as_str(), Some("r1"));

    let calls = tmux.get_calls();
    let kill_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"kill-pane".to_string()))
        .collect();
    assert!(!kill_calls.is_empty(), "kill-pane should have been called");
    assert!(kill_calls[0].contains(&"%5".to_string()));
}

#[tokio::test]
async fn terminate_agent_unknown_name_errors() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%1");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let result = handle_terminate_agent(
        &serde_json::json!({ "name": "nonexistent" }),
        &mut state,
        &tmux,
    );
    assert!(result.is_err(), "Should error for unknown agent name");
    assert!(result.unwrap_err().message.contains("nonexistent"));
}

#[tokio::test]
async fn terminate_agent_clears_window_when_empty() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%7")
        .with_window("@7")
        .with_list_panes_response("");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    handle_spawn_agent(
        &serde_json::json!({ "agent": "coder", "name": "c1" }),
        &mut state,
        &tmux,
    )
    .expect("spawn failed");
    handle_terminate_agent(&serde_json::json!({ "name": "c1" }), &mut state, &tmux)
        .expect("terminate failed");

    assert!(
        state.tmux_window.is_none(),
        "tmux_window should be None when no panes remain"
    );
}

#[tokio::test]
async fn terminate_agent_removes_from_panes_map() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%8")
        .with_window("@8")
        .with_list_panes_response("%99\n");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher", "name": "r2" }),
        &mut state,
        &tmux,
    )
    .expect("spawn failed");
    assert!(state.agent_panes.contains_key("r2"));

    handle_terminate_agent(&serde_json::json!({ "name": "r2" }), &mut state, &tmux)
        .expect("terminate failed");
    assert!(!state.agent_panes.contains_key("r2"));
    assert!(
        state.tmux_window.is_some(),
        "window should remain when other panes exist"
    );
}

// ============================================================================
// Window discovery tests
// ============================================================================

#[tokio::test]
async fn handle_spawn_agent_discovers_existing_agents_window() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%50").with_list_windows_response("@99\tagents\n");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let result = handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    );
    assert!(
        result.is_ok(),
        "Should succeed by discovering existing agents window"
    );

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
    assert!(cmd_names.contains(&"list-windows"));
    assert!(
        cmd_names.contains(&"split-window"),
        "Should split into discovered window"
    );
    assert!(
        !cmd_names.contains(&"new-window"),
        "Should NOT create new-window"
    );

    let split_calls: Vec<_> = calls.iter().filter(|c| c[0] == "split-window").collect();
    assert!(split_calls[0].contains(&"@99".to_string()));
    assert_eq!(state.tmux_window.as_deref(), Some("@99"));
}

#[tokio::test]
async fn handle_spawn_agent_ignores_non_agents_windows() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%51").with_list_windows_response("@1\tshell\n@2\tcode\n");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let result = handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    );
    assert!(result.is_ok(), "Should succeed by creating new window");

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
    assert!(cmd_names.contains(&"list-windows"));
    assert!(
        cmd_names.contains(&"new-window"),
        "Should create new-window when no 'agents' window found"
    );
}

#[tokio::test]
async fn handle_spawn_agent_fallsthrough_when_discovered_split_fails() {
    struct DiscoverySplitFailRunner {
        calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    }

    impl DiscoverySplitFailRunner {
        fn get_calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl TmuxRunner for DiscoverySplitFailRunner {
        fn run(&self, args: &[&str]) -> Result<String, ToolHandlerError> {
            let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            self.calls.lock().unwrap().push(args_owned.clone());
            if args_owned.contains(&"list-windows".to_string()) {
                return Ok("@99\tagents\n".to_string());
            }
            if args_owned.contains(&"split-window".to_string()) {
                return Err(ToolHandlerError::runtime("can't find window: @99"));
            }
            if args_owned.contains(&"new-window".to_string()) {
                return Ok("@new\t%60\n".to_string());
            }
            if args_owned.contains(&"display-message".to_string()) {
                return Ok("nu\n".to_string());
            }
            Ok(String::new())
        }
    }

    let _state_dir = TempDir::new().unwrap();
    let tmux = DiscoverySplitFailRunner {
        calls: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());

    let result = handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    );
    assert!(result.is_ok(), "Should fallback to new-window: {result:?}");

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
    assert!(cmd_names.contains(&"list-windows"));
    assert!(cmd_names.contains(&"split-window"));
    assert!(
        cmd_names.contains(&"new-window"),
        "Should fallback to new-window after split failure"
    );
    assert_eq!(state.tmux_window.as_deref(), Some("@new"));
}

#[tokio::test]
async fn handle_spawn_agent_skips_discovery_when_tmux_window_set() {
    let _state_dir = TempDir::new().unwrap();
    let tmux = MockTmuxRunner::new("%70").with_list_windows_response("@99\tagents\n");
    let mut state = OrchestratorState::new(_state_dir.path().to_path_buf());
    state.tmux_window = Some("@existing".to_string());

    let result = handle_spawn_agent(
        &serde_json::json!({ "agent": "researcher" }),
        &mut state,
        &tmux,
    );
    assert!(result.is_ok(), "Should succeed using existing window");

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
    assert!(
        !cmd_names.contains(&"list-windows"),
        "Should NOT call list-windows when window already set"
    );
    assert!(
        cmd_names.contains(&"split-window"),
        "Should split into existing window"
    );

    let split_calls: Vec<_> = calls.iter().filter(|c| c[0] == "split-window").collect();
    assert!(split_calls[0].contains(&"@existing".to_string()));
}
