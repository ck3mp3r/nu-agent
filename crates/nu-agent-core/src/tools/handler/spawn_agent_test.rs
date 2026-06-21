use std::sync::Arc;
use tokio::sync::RwLock;

use crate::mailbox::AgentRegistry;

use super::spawn_agent::{
    OrchestratorState, TmuxRunner, generate_hex_token, handle_spawn_agent,
    handle_terminate_agent,
};
use super::ToolHandlerError;

/// Mock TmuxRunner for testing - thread-safe version
/// Returns `pane_response` for new-window/split-window and `shell_response` for display-message.
/// All other calls return empty string.
struct MockTmuxRunner {
    calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    window_response: String,
    pane_response: String,
    /// Sequence of responses for display-message calls; cycles last value once exhausted
    display_responses: Vec<String>,
    display_call_count: Arc<std::sync::Mutex<usize>>,
    /// Response for list-panes calls (empty means no panes remain)
    list_panes_response: String,
    /// Response for list-windows calls (None means return empty string)
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

#[tokio::test]
async fn generate_hex_token_correct_length() {
    let token = generate_hex_token(32);
    assert_eq!(token.len(), 64, "32 bytes should produce 64 hex characters");

    // Verify it's valid hex
    for c in token.chars() {
        assert!(
            c.is_ascii_hexdigit(),
            "Token should contain only hex digits"
        );
    }
}

#[tokio::test]
async fn generate_hex_token_uniqueness() {
    let token1 = generate_hex_token(32);
    let token2 = generate_hex_token(32);
    assert_ne!(
        token1, token2,
        "Consecutive tokens should be unique (extremely high probability)"
    );
}

#[tokio::test]
async fn handle_spawn_agent_lazy_inits_broker() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({
        "agent": "researcher",
    });

    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(result.is_ok(), "First spawn should succeed");
    assert!(state.broker.is_some(), "Broker should be initialized");
    assert!(
        state.socket_path.is_some(),
        "Socket path should be initialized"
    );
    assert_eq!(state.spawn_count, 1, "Spawn count should be incremented");
}

#[tokio::test]
async fn handle_spawn_agent_reuses_broker() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // First spawn
    let args1 = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args1, &mut state, &tmux).expect("First spawn failed");

    let broker_ptr_1 = state.broker.as_ref().map(|b| b as *const _);

    // Second spawn
    let args2 = serde_json::json!({ "agent": "coder" });
    handle_spawn_agent(&args2, &mut state, &tmux).expect("Second spawn failed");

    let broker_ptr_2 = state.broker.as_ref().map(|b| b as *const _);

    assert_eq!(
        broker_ptr_1, broker_ptr_2,
        "Second spawn should reuse existing broker"
    );
    assert_eq!(state.spawn_count, 2, "Spawn count should be 2");
}

#[tokio::test]
async fn handle_spawn_agent_auto_names() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // First spawn without name
    let args1 = serde_json::json!({ "agent": "researcher" });
    let result1 = handle_spawn_agent(&args1, &mut state, &tmux).expect("First spawn failed");
    let name1 = result1["name"].as_str().expect("name should be a string");
    assert_eq!(name1, "researcher-1", "First auto-generated name incorrect");

    // Second spawn without name
    let args2 = serde_json::json!({ "agent": "researcher" });
    let result2 = handle_spawn_agent(&args2, &mut state, &tmux).expect("Second spawn failed");
    let name2 = result2["name"].as_str().expect("name should be a string");
    assert_eq!(
        name2, "researcher-2",
        "Second auto-generated name incorrect"
    );
}

#[tokio::test]
async fn handle_spawn_agent_explicit_name() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({
        "agent": "researcher",
        "name": "my-researcher"
    });

    let result = handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");
    let name = result["name"].as_str().expect("name should be a string");
    assert_eq!(name, "my-researcher", "Explicit name not used");
}

#[tokio::test]
async fn handle_spawn_agent_registers_token() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({
        "agent": "researcher",
        "name": "test-agent"
    });

    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    // Note: We can't directly verify the token is in the registry without exposing
    // internal registry state. This test verifies the operation succeeds without
    // errors. A more comprehensive test would require registry inspection APIs.
}

#[tokio::test]
async fn handle_spawn_agent_creates_tmux_window() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls_vec = tmux.get_calls();
    assert!(
        !calls_vec.is_empty(),
        "Tmux should have been called at least once"
    );

    // First call should be list-windows (discovery), second should be new-window
    let first_call = &calls_vec[0];
    assert!(
        first_call.contains(&"list-windows".to_string()),
        "First tmux call should be list-windows for discovery"
    );

    let second_call = &calls_vec[1];
    assert!(
        second_call.contains(&"new-window".to_string()),
        "Second tmux call should create a new window"
    );

    // Third call should be select-layout even-horizontal
    let third_call = &calls_vec[2];
    assert!(
        third_call.contains(&"select-layout".to_string())
            && third_call.contains(&"even-horizontal".to_string()),
        "Third tmux call should select-layout even-horizontal, got: {:?}",
        third_call
    );
}

#[tokio::test]
async fn handle_spawn_agent_splits_pane_on_second_spawn() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // First spawn
    let args1 = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args1, &mut state, &tmux).expect("First spawn failed");

    // Second spawn
    let args2 = serde_json::json!({ "agent": "coder" });
    handle_spawn_agent(&args2, &mut state, &tmux).expect("Second spawn failed");

    let calls_vec = tmux.get_calls();

    // Find split-window call and verify -h flag
    let split_calls: Vec<_> = calls_vec
        .iter()
        .filter(|call| call.contains(&"split-window".to_string()))
        .collect();

    assert!(
        !split_calls.is_empty(),
        "Second spawn should split the window, not create a new one"
    );
    assert!(
        split_calls[0].contains(&"-h".to_string()),
        "split-window should use -h flag for horizontal split, got: {:?}",
        split_calls[0]
    );

    // Verify select-layout is called after split
    let layout_calls: Vec<_> = calls_vec
        .iter()
        .filter(|call| call.contains(&"select-layout".to_string()))
        .collect();
    assert!(
        !layout_calls.is_empty(),
        "Should call select-layout after split-window"
    );
    assert!(
        layout_calls[0].contains(&"even-horizontal".to_string()),
        "select-layout should use even-horizontal"
    );
}

#[tokio::test]
async fn handle_spawn_agent_sends_command() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({
        "agent": "researcher",
        "name": "test-agent"
    });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls_vec = tmux.get_calls();

    // Find send-keys call
    let send_keys_calls: Vec<_> = calls_vec
        .iter()
        .filter(|call| call.contains(&"send-keys".to_string()))
        .collect();

    assert!(
        !send_keys_calls.is_empty(),
        "Should send command to tmux pane"
    );

    // Verify the command contains expected parts (accounting for shell escaping)
    let send_keys_call = &send_keys_calls[0];
    let command = send_keys_call.join(" ");
    assert!(command.contains("agent"), "Command should contain 'agent'");
    assert!(
        command.contains("--agent") && command.contains("'researcher'"),
        "Command should contain '--agent' and 'researcher' (shell-escaped)"
    );
    assert!(
        command.contains("--name") && command.contains("'test-agent'"),
        "Command should contain '--name' and 'test-agent' (shell-escaped)"
    );
    assert!(
        command.contains("--broker-socket"),
        "Command should contain '--broker-socket'"
    );
    assert!(
        command.contains("--broker-token"),
        "Command should contain '--broker-token'"
    );
}

#[tokio::test]
async fn handle_spawn_agent_includes_parent_name_from_identity() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());
    state.agent_identity = Some("my-orchestrator".to_string());

    let args = serde_json::json!({
        "agent": "researcher",
        "name": "test-agent"
    });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls_vec = tmux.get_calls();
    let send_keys_calls: Vec<_> = calls_vec
        .iter()
        .filter(|call| call.contains(&"send-keys".to_string()))
        .collect();

    assert!(
        !send_keys_calls.is_empty(),
        "Should send command to tmux pane"
    );
    let command = send_keys_calls[0].join(" ");
    assert!(
        command.contains("--parent-name") && command.contains("'my-orchestrator'"),
        "Command should contain '--parent-name' with orchestrator identity, got: {command}"
    );
}

#[tokio::test]
async fn handle_spawn_agent_defaults_parent_name_to_orchestrator() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("@1".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());
    // agent_identity is None — should default to "orchestrator"

    let args = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls_vec = tmux.get_calls();
    let send_keys_calls: Vec<_> = calls_vec
        .iter()
        .filter(|call| call.contains(&"send-keys".to_string()))
        .collect();

    let command = send_keys_calls[0].join(" ");
    assert!(
        command.contains("--parent-name") && command.contains("'orchestrator'"),
        "Command should default --parent-name to 'orchestrator', got: {command}"
    );
}

/// Mock TmuxRunner that fails on split-window but succeeds on new-window and send-keys
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
        // send-keys and anything else succeeds
        Ok(String::new())
    }
}

#[tokio::test]
async fn handle_spawn_agent_recovers_from_stale_tmux_window() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = StaleTmuxRunner::new("@2".to_string());
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Simulate stale window reference
    state.tmux_window = Some("@dead".to_string());

    // Pre-init broker so test focuses on tmux recovery
    let broker = crate::mailbox::Broker::start(Arc::clone(&registry)).expect("broker should start");
    state.socket_path = Some(broker.socket_path().to_path_buf());
    state.broker = Some(broker);

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(
        result.is_ok(),
        "Spawn should recover from stale window: {result:?}"
    );

    // Window should be updated to new id
    assert_eq!(
        state.tmux_window.as_deref(),
        Some("@2"),
        "tmux_window should be set to new window id"
    );

    let calls = tmux.get_calls();

    // Should have attempted split-window, then list-windows (discovery), then new-window, then send-keys
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
        "Should attempt split, discover via list-windows, fallback to new-window, select-layout, display-message, then send-keys"
    );
}

#[tokio::test]
async fn shell_ready_immediately() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%1");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
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
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%2").with_display_responses(vec![
        "direnv\n".to_string(),
        "direnv\n".to_string(),
        "nu\n".to_string(),
    ]);
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(result.is_ok(), "Should succeed after delayed readiness");

    let calls = tmux.get_calls();
    let display_calls: Vec<_> = calls.iter().filter(|c| c[0] == "display-message").collect();
    assert_eq!(
        display_calls.len(),
        3,
        "Should have polled 3 times before ready"
    );

    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert!(
        !send_keys.is_empty(),
        "send-keys should be called after ready"
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

    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = NeverReadyRunner {
        calls: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Pre-init broker
    let broker = crate::mailbox::Broker::start(Arc::clone(&registry)).expect("broker");
    state.socket_path = Some(broker.socket_path().to_path_buf());
    state.broker = Some(broker);

    // Call wait_for_shell_ready directly with a short timeout
    let result = super::spawn_agent::wait_for_shell_ready_pub(
        &tmux,
        "%3",
        std::time::Duration::from_millis(100),
    );
    assert!(result.is_err(), "Should timeout when shell never ready");
    let msg = result.unwrap_err().message;
    assert!(
        msg.contains("Shell not ready"),
        "Error should mention timeout: {msg}"
    );

    let calls = tmux.calls.lock().unwrap().clone();
    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert!(
        send_keys.is_empty(),
        "send-keys should not be called on timeout"
    );
}

#[tokio::test]
async fn pane_id_captured_and_used() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%10").with_window("@10");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args, &mut state, &tmux).expect("Spawn failed");

    let calls = tmux.get_calls();

    let new_window: Vec<_> = calls.iter().filter(|c| c[0] == "new-window").collect();
    assert!(!new_window.is_empty());
    assert!(
        new_window[0].contains(&"#{window_id}\t#{pane_id}".to_string()),
        "new-window should request both window_id and pane_id"
    );

    let display: Vec<_> = calls.iter().filter(|c| c[0] == "display-message").collect();
    assert!(!display.is_empty());
    assert!(
        display[0].contains(&"%10".to_string()),
        "display-message should target captured pane_id"
    );

    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert!(!send_keys.is_empty());
    assert!(
        send_keys[0].contains(&"%10".to_string()),
        "send-keys should target captured pane_id"
    );

    assert_eq!(
        state.tmux_window.as_deref(),
        Some("@10"),
        "tmux_window should hold the window ID"
    );
}

#[tokio::test]
async fn split_window_captures_pane_id() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%20");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // First spawn creates window
    let args1 = serde_json::json!({ "agent": "researcher" });
    handle_spawn_agent(&args1, &mut state, &tmux).expect("First spawn failed");

    // Second spawn uses split-window
    let args2 = serde_json::json!({ "agent": "coder" });
    handle_spawn_agent(&args2, &mut state, &tmux).expect("Second spawn failed");

    let calls = tmux.get_calls();

    let splits: Vec<_> = calls.iter().filter(|c| c[0] == "split-window").collect();
    assert!(!splits.is_empty());
    assert!(
        splits[0].contains(&"#{pane_id}".to_string()),
        "split-window should request pane_id format"
    );

    // Find the second set of send-keys (after split)
    let send_keys: Vec<_> = calls.iter().filter(|c| c[0] == "send-keys").collect();
    assert_eq!(send_keys.len(), 2, "Two send-keys calls expected");
    assert!(
        send_keys[1].contains(&"%20".to_string()),
        "Second send-keys should target split pane_id"
    );
}

// ============================================================================
// terminate_agent tests
// ============================================================================

#[tokio::test]
async fn terminate_agent_kills_pane() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%5")
        .with_window("@5")
        .with_list_panes_response("%99\n"); // another pane still alive
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Spawn an agent first
    let spawn_args = serde_json::json!({ "agent": "researcher", "name": "r1" });
    handle_spawn_agent(&spawn_args, &mut state, &tmux).expect("spawn failed");

    // Terminate it
    let term_args = serde_json::json!({ "name": "r1" });
    let result = handle_terminate_agent(&term_args, &mut state, &tmux).expect("terminate failed");

    // Verify return value
    assert_eq!(result["terminated"].as_str(), Some("r1"));

    // Verify kill-pane was called with the correct pane_id
    let calls = tmux.get_calls();
    let kill_calls: Vec<_> = calls
        .iter()
        .filter(|c| c.contains(&"kill-pane".to_string()))
        .collect();
    assert!(!kill_calls.is_empty(), "kill-pane should have been called");
    assert!(
        kill_calls[0].contains(&"%5".to_string()),
        "kill-pane should target pane_id %5, got: {:?}",
        kill_calls[0]
    );
}

#[tokio::test]
async fn terminate_agent_unknown_name_errors() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%1");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Terminate without spawning
    let term_args = serde_json::json!({ "name": "nonexistent" });
    let result = handle_terminate_agent(&term_args, &mut state, &tmux);

    assert!(result.is_err(), "Should error for unknown agent name");
    let err = result.unwrap_err();
    assert!(
        err.message.contains("nonexistent"),
        "Error should mention the agent name, got: {}",
        err.message
    );
}

#[tokio::test]
async fn terminate_agent_clears_window_when_empty() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    // list-panes returns empty — no panes remain after kill
    let tmux = MockTmuxRunner::new("%7")
        .with_window("@7")
        .with_list_panes_response("");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Spawn
    let spawn_args = serde_json::json!({ "agent": "coder", "name": "c1" });
    handle_spawn_agent(&spawn_args, &mut state, &tmux).expect("spawn failed");
    assert!(
        state.tmux_window.is_some(),
        "Window should be set after spawn"
    );

    // Terminate — list-panes returns empty
    let term_args = serde_json::json!({ "name": "c1" });
    handle_terminate_agent(&term_args, &mut state, &tmux).expect("terminate failed");

    assert!(
        state.tmux_window.is_none(),
        "tmux_window should be None when no panes remain"
    );
}

#[tokio::test]
async fn terminate_agent_removes_from_panes_map() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%8")
        .with_window("@8")
        .with_list_panes_response("%99\n"); // other panes remain
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Spawn
    let spawn_args = serde_json::json!({ "agent": "researcher", "name": "r2" });
    handle_spawn_agent(&spawn_args, &mut state, &tmux).expect("spawn failed");
    assert!(
        state.agent_panes.contains_key("r2"),
        "agent_panes should contain 'r2' after spawn"
    );

    // Terminate
    let term_args = serde_json::json!({ "name": "r2" });
    handle_terminate_agent(&term_args, &mut state, &tmux).expect("terminate failed");

    assert!(
        !state.agent_panes.contains_key("r2"),
        "agent_panes should not contain 'r2' after terminate"
    );
    // Window should still exist since list-panes returned a pane
    assert!(
        state.tmux_window.is_some(),
        "tmux_window should remain when other panes exist"
    );
}

// ============================================================================
// Window discovery tests
// ============================================================================

#[tokio::test]
async fn handle_spawn_agent_discovers_existing_agents_window() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%50").with_list_windows_response("@99\tagents\n");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(
        result.is_ok(),
        "Should succeed by discovering existing agents window"
    );

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();

    assert!(
        cmd_names.contains(&"list-windows"),
        "Should call list-windows for discovery"
    );
    assert!(
        cmd_names.contains(&"split-window"),
        "Should split into discovered window"
    );
    assert!(
        !cmd_names.contains(&"new-window"),
        "Should NOT create new-window when discovery succeeds"
    );

    // split-window should target @99
    let split_calls: Vec<_> = calls.iter().filter(|c| c[0] == "split-window").collect();
    assert!(
        split_calls[0].contains(&"@99".to_string()),
        "split-window should target discovered window @99, got: {:?}",
        split_calls[0]
    );

    assert_eq!(
        state.tmux_window.as_deref(),
        Some("@99"),
        "tmux_window should be set to discovered window id"
    );
}

#[tokio::test]
async fn handle_spawn_agent_ignores_non_agents_windows() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%51").with_list_windows_response("@1\tshell\n@2\tcode\n");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(result.is_ok(), "Should succeed by creating new window");

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();

    assert!(
        cmd_names.contains(&"list-windows"),
        "Should call list-windows for discovery"
    );
    assert!(
        cmd_names.contains(&"new-window"),
        "Should create new-window when no 'agents' window found"
    );
}

#[tokio::test]
async fn handle_spawn_agent_fallsthrough_when_discovered_split_fails() {
    // Custom mock: list-windows succeeds with "@99\tagents", but split-window
    // targeting @99 fails. The fallback should create a new window.
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

    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = DiscoverySplitFailRunner {
        calls: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Pre-init broker to focus on tmux logic
    let broker = crate::mailbox::Broker::start(Arc::clone(&registry)).expect("broker should start");
    state.socket_path = Some(broker.socket_path().to_path_buf());
    state.broker = Some(broker);

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(result.is_ok(), "Should fallback to new-window: {result:?}");

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();

    assert!(
        cmd_names.contains(&"list-windows"),
        "Should attempt discovery"
    );
    assert!(
        cmd_names.contains(&"split-window"),
        "Should attempt split into discovered window"
    );
    assert!(
        cmd_names.contains(&"new-window"),
        "Should fallback to new-window after split failure"
    );

    assert_eq!(
        state.tmux_window.as_deref(),
        Some("@new"),
        "tmux_window should be set to new window id after fallback"
    );
}

#[tokio::test]
async fn handle_spawn_agent_skips_discovery_when_tmux_window_set() {
    let registry = Arc::new(RwLock::new(AgentRegistry::new()));
    let tmux = MockTmuxRunner::new("%70").with_list_windows_response("@99\tagents\n");
    let mut state = OrchestratorState::new(Arc::clone(&registry), std::env::temp_dir());

    // Pre-init broker
    let broker = crate::mailbox::Broker::start(Arc::clone(&registry)).expect("broker should start");
    state.socket_path = Some(broker.socket_path().to_path_buf());
    state.broker = Some(broker);

    // Set existing tmux_window — discovery should be skipped
    state.tmux_window = Some("@existing".to_string());

    let args = serde_json::json!({ "agent": "researcher" });
    let result = handle_spawn_agent(&args, &mut state, &tmux);
    assert!(result.is_ok(), "Should succeed using existing window");

    let calls = tmux.get_calls();
    let cmd_names: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();

    assert!(
        !cmd_names.contains(&"list-windows"),
        "Should NOT call list-windows when tmux_window is already set"
    );
    assert!(
        cmd_names.contains(&"split-window"),
        "Should split into existing window"
    );

    // split-window should target @existing
    let split_calls: Vec<_> = calls.iter().filter(|c| c[0] == "split-window").collect();
    assert!(
        split_calls[0].contains(&"@existing".to_string()),
        "split-window should target @existing, got: {:?}",
        split_calls[0]
    );
}
