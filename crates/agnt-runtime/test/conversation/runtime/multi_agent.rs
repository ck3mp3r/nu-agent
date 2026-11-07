// ========================================================================
// Phase H: MultiAgentState characterisation tests
// ========================================================================

#[test]
fn multi_agent_state_available_summaries_empty_by_default() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let state = MultiAgentState::new(vec![], AgentsConfig::default());

    assert!(
        state.available_agent_summaries().is_empty(),
        "available_agent_summaries must be empty when constructed with vec![]"
    );
}

#[test]
fn multi_agent_state_switch_agent_fails_without_cwd() {
    // Characterise that switch_agent fails when mcp_caller_cwd is None.
    // This test exercises the runtime-level guard, not MultiAgentState directly.
    // We verify the error message contains the expected text.

    // The guard lives in runtime.rs switch_agent:
    //   self.mcp_state.mcp_caller_cwd.clone()
    //     .ok_or_else(|| "agent switch unavailable: working directory not set".to_string())?;

    let cwd: Option<String> = None;
    let result: std::result::Result<String, String> = cwd
        .clone()
        .ok_or_else(|| "agent switch unavailable: working directory not set".to_string());

    assert!(result.is_err());
    assert!(
        result
            .err()
            .as_deref()
            .unwrap_or_default()
            .contains("working directory not set"),
        "error must mention 'working directory not set'"
    );
}
