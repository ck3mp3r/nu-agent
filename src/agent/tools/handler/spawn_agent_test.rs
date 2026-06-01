use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agent::mailbox::AgentRegistry;

use super::spawn_agent::{
    generate_hex_token, handle_spawn_agent, OrchestratorState, TmuxRunner, ToolExecError,
};

/// Mock TmuxRunner for testing - thread-safe version
struct MockTmuxRunner {
    calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    response: String,
}

impl MockTmuxRunner {
    fn new(response: String) -> Self {
        Self {
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
            response,
        }
    }

    fn get_calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockTmuxRunner {
    fn run(&self, args: &[&str]) -> Result<String, ToolExecError> {
        let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.calls.lock().unwrap().push(args_owned);
        Ok(self.response.clone())
    }
}

#[tokio::test]
async fn generate_hex_token_correct_length() {
    let token = generate_hex_token(32);
    assert_eq!(token.len(), 64, "32 bytes should produce 64 hex characters");

    // Verify it's valid hex
    for c in token.chars() {
        assert!(c.is_ascii_hexdigit(), "Token should contain only hex digits");
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

    // First call should be new-window
    let first_call = &calls_vec[0];
    assert!(
        first_call.contains(&"new-window".to_string()),
        "First tmux call should create a new window"
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

    // Find split-window call
    let split_calls: Vec<_> = calls_vec
        .iter()
        .filter(|call| call.contains(&"split-window".to_string()))
        .collect();

    assert!(
        !split_calls.is_empty(),
        "Second spawn should split the window, not create a new one"
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
    assert!(
        command.contains("agent"),
        "Command should contain 'agent'"
    );
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

    assert!(!send_keys_calls.is_empty(), "Should send command to tmux pane");
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
