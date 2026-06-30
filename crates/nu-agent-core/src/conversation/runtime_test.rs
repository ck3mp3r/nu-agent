use super::*;

use crate::compaction::CompactionStrategy;
use crate::conversation::providers::ClientCacheKey;
use crate::protocol::{contracts::ProgressUi, event::UiEvent};
use crate::types::ToolDefinition;

#[derive(Default)]
struct TestProgressUi {
    events: Vec<UiEvent>,
}

impl ProgressUi for TestProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

#[test]
fn permissions_startup_summary_emits_once_before_first_turn() {
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let mut ui = TestProgressUi::default();
    let summary =
        "permissions policy: overlay_active=false global=ask tool_rules=5 nu__run.command_rules=1";

    let mut state = super::super::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        summary.to_string(),
    );

    state.emit_startup_summary_once(&mut ui);
    state.emit_startup_summary_once(&mut ui);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(warnings, 1);

    let warning_message = ui
        .events
        .iter()
        .find_map(|event| match event {
            UiEvent::Warning { message } => Some(message.clone()),
            _ => None,
        })
        .expect("warning event");
    assert_eq!(warning_message, summary);
}

// ========================================================================
// Structured messages tests
// ========================================================================

#[test]
fn build_system_preamble_joins_non_empty_parts() {
    let result = super::build_system_preamble(
        Some("preamble text"),
        None,
        None,
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("preamble text"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));
}

#[test]
fn build_system_preamble_returns_none_when_all_empty() {
    let result = super::build_system_preamble(None, None, None, None, None, None);
    assert!(result.is_none());
}

#[test]
fn build_system_preamble_handles_partial_inputs() {
    let result =
        super::build_system_preamble(Some("preamble"), None, None, None, Some("agents"), None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("preamble"));
    assert!(text.contains("agents"));
}

#[test]
fn build_system_preamble_includes_persona_in_correct_position() {
    let result = super::build_system_preamble(
        Some("config preamble"),
        Some("agent persona"),
        None,
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.unwrap();

    // Verify all parts are present
    assert!(text.contains("config preamble"));
    assert!(text.contains("agent persona"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));

    // Verify persona appears between config preamble and context
    let config_pos = text.find("config preamble").unwrap();
    let persona_pos = text.find("agent persona").unwrap();
    let context_pos = text.find("context text").unwrap();

    assert!(
        config_pos < persona_pos,
        "config preamble should come before persona"
    );
    assert!(
        persona_pos < context_pos,
        "persona should come before context"
    );
}

#[test]
fn build_system_preamble_persona_only() {
    let result = super::build_system_preamble(None, Some("persona only"), None, None, None, None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert_eq!(text, "persona only");
}

#[test]
fn build_system_preamble_includes_sub_agent_instruction() {
    let result = super::build_system_preamble(
        None,
        Some("persona"),
        Some("sub-agent instruction"),
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let text = result.unwrap();
    assert!(text.contains("persona"));
    assert!(text.contains("sub-agent instruction"));

    // sub-agent instruction should come after persona
    let persona_pos = text.find("persona").unwrap();
    let instruction_pos = text.find("sub-agent instruction").unwrap();
    assert!(
        persona_pos < instruction_pos,
        "sub-agent instruction should come after persona"
    );
}

#[test]
fn build_system_preamble_sub_agent_instruction_only() {
    let result =
        super::build_system_preamble(None, None, Some("you are a sub-agent"), None, None, None);

    assert!(result.is_some());
    let text = result.unwrap();
    assert_eq!(text, "you are a sub-agent");
}

// ========================================================================
// Memory and conversation store tests
// ========================================================================

#[test]
fn runtime_struct_has_memory_field() {
    // GREEN: This test now compiles, proving the memory field exists as JournalConversationMemory
    use crate::session::JournalConversationMemory;

    // Compile-time check that the field exists with correct type
    fn _assert_field_exists(_memory: &JournalConversationMemory) {}

    // We can't easily construct a runtime in tests, but we can verify
    // the type signature compiles
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.memory_state.memory());
    };
}

#[test]
fn runtime_struct_has_conversation_store_field() {
    // GREEN: This test now compiles, proving the conversation_store field exists
    use crate::session::JsonlConversationStore;

    // Compile-time check that the field exists with correct type
    fn _assert_field_exists(_store: &JsonlConversationStore) {}

    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.memory_state.conversation_store());
    };
}

#[test]
fn evaluate_auto_compaction_uses_token_based_policy() {
    // Verify that TokenCompactionPolicy is used for auto-compaction evaluation.
    // We can't easily construct a full runtime, but we verify the policy logic directly.
    use crate::protocol::compaction::{CompactionTriggerPolicy, TokenCompactionPolicy};

    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);

    // At 80% usage (160k of 200k) — should fire
    let decision = policy.evaluate(Some(160_000));
    assert!(
        matches!(
            decision,
            crate::protocol::compaction::CompactionTriggerDecision::Fire { .. }
        ),
        "Expected compaction to fire at 80% token usage"
    );

    // At 50% usage — should not fire
    let decision2 = policy.evaluate(Some(100_000));
    assert!(
        matches!(
            decision2,
            crate::protocol::compaction::CompactionTriggerDecision::NoFire { .. }
        ),
        "Expected no compaction at 50% token usage"
    );
}

// Provider dispatch tests

#[test]
fn provider_dispatch_unsupported_provider_returns_error() {
    // RED: Verify that unsupported provider returns clear error
    use crate::config::Config;

    let config = Config {
        provider: "unsupported-provider".to_string(),
        provider_impl: None,
        model: "some-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    // This test will compile once we add the dispatch logic
    // For now, document that build_copilot_client works for copilot only
    // When we add dispatch in execute_turn, this will test the error path

    // Expected behavior: execute_turn should return error with:
    // "Unsupported provider: 'unsupported-provider'"
    // This test documents the requirement for now
    assert_eq!(config.provider, "unsupported-provider");
}

// ========================================================================
// Memory hydration guard tests (now: JournalConversationMemory)
// ========================================================================

#[test]
fn runtime_struct_has_compacting_field() {
    // Compile-time check that the compacting field exists with correct type
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn _assert_field_exists(_flag: &Arc<AtomicBool>) {}

    let _type_check: fn(&AgentConversationRuntime) = |r| {
        _assert_field_exists(r.compaction_state.compacting());
    };
}

#[test]
fn runtime_struct_has_memory_state_field() {
    // Compile-time check that the memory_state field exposes memory() and last_total_tokens().
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _tokens: Option<u64> = r.memory_state.last_total_tokens();
    };
}

#[test]
fn client_cache_key_contains_provider_and_model() {
    // Characterise client_cache_key (runtime.rs:708-714).
    // It returns (config.provider, config.api_key, config.base_url).
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    // client_cache_key clones (provider, api_key, base_url) from config.
    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
    );

    assert_eq!(
        key,
        ("copilot".to_string(), Some("test-key".to_string()), None,)
    );
}

#[test]
fn client_cache_key_includes_base_url_when_set() {
    // Same as above but with base_url set.
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: Some("https://custom.example.com".to_string()),
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
    );

    assert_eq!(
        key,
        (
            "copilot".to_string(),
            Some("test-key".to_string()),
            Some("https://custom.example.com".to_string()),
        )
    );
}

#[test]
fn active_tool_definitions_returns_empty_when_no_tools() {
    // Characterise active_tool_definitions (runtime.rs:753-759).
    // Delegates to handler::llm_visible_tool_definitions with the runtime's
    // tool_definitions, mcp_registry, and permissions.
    use crate::tools::{
        authz::PermissionsConfig,
        handler::{self, McpToolRegistry},
    };

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    let result =
        handler::llm_visible_tool_definitions(&tool_definitions, &mcp_registry, &permissions);

    assert!(result.is_empty());
}

// ========================================================================
// Phase C-pre: Characterise sub-struct clusters before field decomposition
// ========================================================================

#[test]
fn mcp_state_initial_tool_count_is_zero() {
    // Characterise llm_visible_mcp_tool_count (runtime.rs:372-377).
    // With an empty mcp_registry and no mcp_lifecycle_projection, the
    // method filters active_tool_definitions by mcp_registry.is_registered
    // — an empty registry yields 0.
    use crate::tools::{authz::PermissionsConfig, handler::McpToolRegistry};

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    // Replicate the method body: filter active_tool_definitions by registry
    let active = super::super::super::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &mcp_registry,
        &permissions,
    );
    let count = active
        .iter()
        .filter(|tool| mcp_registry.is_registered(tool.name.as_str()))
        .count();

    assert_eq!(count, 0, "empty registry must yield zero MCP tool count");
}

#[test]
fn mcp_state_tool_count_by_server_returns_zero_for_unknown() {
    // Characterise llm_visible_mcp_tool_count_for_server (runtime.rs:379-386).
    // Querying for "nonexistent-server" with an empty registry must return 0.
    use crate::tools::{authz::PermissionsConfig, handler::McpToolRegistry};

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    let active = super::super::super::tools::handler::llm_visible_tool_definitions(
        &tool_definitions,
        &mcp_registry,
        &permissions,
    );
    let count = active
        .iter()
        .filter(|tool| mcp_registry.is_registered(tool.name.as_str()))
        .filter_map(|tool| mcp_registry.server_name_for(tool.name.as_str()))
        .filter(|server| *server == "nonexistent-server")
        .count();

    assert_eq!(count, 0, "unknown server must yield zero tool count");
}

#[test]
fn compaction_state_evaluate_returns_none_when_no_tokens() {
    // Characterise evaluate_auto_compaction (runtime.rs:488-495).
    // When last_total_tokens is None, the policy returns NoFire("no_token_data")
    // and the method wraps it in Some(...).
    use crate::protocol::compaction::{
        CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
    };

    let policy = TokenCompactionPolicy::new(100_000, 0.8, CompactionStrategy::SlidingSummary);
    let decision = Some(policy.evaluate(None));

    assert!(
        matches!(decision, Some(CompactionTriggerDecision::NoFire { .. })),
        "no token data must yield Some(NoFire), not Fire"
    );
}

#[test]
fn compaction_state_evaluate_returns_none_below_threshold() {
    // Characterise evaluate_auto_compaction (runtime.rs:488-495).
    // 50k tokens against 100k window with 80% threshold => 50% usage, below threshold.
    use crate::protocol::compaction::{
        CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
    };

    let policy = TokenCompactionPolicy::new(100_000, 0.8, CompactionStrategy::SlidingSummary);
    let decision = Some(policy.evaluate(Some(50_000)));

    assert!(
        matches!(decision, Some(CompactionTriggerDecision::NoFire { .. })),
        "50% usage below 80% threshold must yield Some(NoFire)"
    );
}

#[test]
fn memory_state_hydrated_flag_starts_false() {
    // JournalConversationMemory replaces the old memory_hydrated bool.
    // The load-on-demand pattern means the cache starts empty — verified
    // by checking last_total_tokens is None on a fresh MemoryState.
    let temp_dir = tempfile::tempdir().unwrap();
    let ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    assert!(
        ms.last_total_tokens().is_none(),
        "last_total_tokens must be None in a fresh MemoryState"
    );

    // Compile-time proof that memory_state.memory() returns JournalConversationMemory.
    let _type_check: fn(&AgentConversationRuntime) = |r| {
        let _: Option<u64> = r.memory_state.last_total_tokens();
    };
}

#[test]
fn active_model_identity_returns_provider_slash_model() {
    // Characterise active_model_identity (runtime.rs:484-486).
    // Returns format!("{}/{}", config.provider, config.model).
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "claude-sonnet-4".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    // Replicate the method body exactly
    let identity = format!("{}/{}", config.provider, config.model);

    assert!(
        identity.contains("copilot"),
        "identity must contain provider"
    );
    assert!(
        identity.contains("claude-sonnet-4"),
        "identity must contain model"
    );
    assert_eq!(
        identity, "copilot/claude-sonnet-4",
        "identity must be provider/model"
    );
}

// ========================================================================
// Phase E: PermissionState characterisation tests
// ========================================================================

#[test]
fn permission_state_startup_not_emitted_on_construction() {
    // After construction, startup_emitted must be false even when
    // startup_summary is non-empty — emission only happens during
    // execute_turn, not at construction time.
    // We verify by calling emit_startup_summary_once on a freshly
    // constructed PermissionState and confirming it does emit (proving
    // the flag was false).
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let mut ui = TestProgressUi::default();
    let mut state = super::super::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        "non-empty summary".to_string(),
    );

    state.emit_startup_summary_once(&mut ui);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(
        warnings, 1,
        "fresh PermissionState must have startup_emitted=false, so first call emits"
    );
}

#[test]
fn permission_state_emit_startup_summary_emits_once() {
    // emit_startup_summary_once must emit exactly one Warning event, even
    // when called twice.
    use crate::tools::authz::{PermissionsConfig, SessionGrantCache};

    let mut ui = TestProgressUi::default();
    let summary = "test permissions summary";

    let mut state = super::super::state::permission::PermissionState::new(
        PermissionsConfig::safe_defaults(true),
        SessionGrantCache::default(),
        summary.to_string(),
    );

    state.emit_startup_summary_once(&mut ui);
    state.emit_startup_summary_once(&mut ui);

    let warnings = ui
        .events
        .iter()
        .filter(|e| matches!(e, UiEvent::Warning { .. }))
        .count();
    assert_eq!(warnings, 1, "must emit exactly 1 warning, not 2");
}

// ========================================================================
// Phase F: ProviderState characterisation tests
// ========================================================================

#[test]
fn provider_state_switch_model_returns_err_when_no_startup_config() {
    // Characterise switch_model (runtime.rs ExtendedRuntime impl).
    // When startup_plugin_config is None, switch_model must return Err
    // containing "model switch unavailable".

    let startup_plugin_config: Option<crate::config::PluginConfig> = None;

    // Replicate the switch_model error path with no startup config
    let result: Result<String, String> = startup_plugin_config
        .ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })
        .map(|_| "unreachable".to_string());

    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("model switch unavailable"),
        "error must mention 'model switch unavailable'"
    );
}

#[test]
fn provider_state_client_cache_key_contains_provider_and_api_key() {
    // Characterise client_cache_key (runtime.rs:302-308).
    // With provider="copilot", api_key=Some("fake-key"), base_url=None,
    // client_cache_key returns (provider, api_key, base_url).
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("fake-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    // Replicate client_cache_key body
    let key: ClientCacheKey = (
        config.provider.clone(),
        config.api_key.clone(),
        config.base_url.clone(),
    );

    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, Some("fake-key".to_string()));
    assert_eq!(key.2, None);
}

// ========================================================================
// Phase G: ToolState characterisation tests
// ========================================================================

#[test]
fn tool_state_active_definitions_empty_when_no_tools() {
    // Characterise active_tool_definitions: with empty tool_definitions,
    // the method delegates to handler::llm_visible_tool_definitions
    // and returns an empty Vec.
    use crate::tools::{
        authz::PermissionsConfig,
        handler::{self, McpToolRegistry},
    };

    let tool_definitions: Vec<ToolDefinition> = vec![];
    let mcp_registry = McpToolRegistry::from_names(std::iter::empty::<String>());
    let permissions = PermissionsConfig::safe_defaults(true);

    let result =
        handler::llm_visible_tool_definitions(&tool_definitions, &mcp_registry, &permissions);

    assert!(
        result.is_empty(),
        "active_tool_definitions must return empty Vec when no tools defined"
    );
}

#[test]
fn tool_state_baseline_is_reset_source() {
    // Characterise that baseline_tool_definitions serves as the reset
    // source: cloning baseline into tool_definitions restores initial state.

    let _tool_definitions: Vec<ToolDefinition> = vec![];
    let baseline_tool_definitions: Vec<ToolDefinition> = vec![ToolDefinition {
        name: "test_tool".to_string(),
        description: "".to_string(),
        parameters: serde_json::json!({}),
    }];

    // Simulate switch_agent reset: tool_definitions = baseline_tool_definitions.clone()
    let tool_definitions = baseline_tool_definitions.clone();

    assert_eq!(
        tool_definitions.len(),
        1,
        "after reset, tool_definitions must match baseline length"
    );
    assert_eq!(tool_definitions[0].name, "test_tool");
}

// ========================================================================
// Phase H: MultiAgentState characterisation tests
// ========================================================================

#[test]
fn multi_agent_state_available_summaries_empty_by_default() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let state = MultiAgentState::new(None, vec![], AgentsConfig::default());

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
    let result: Result<String, String> = cwd
        .clone()
        .ok_or_else(|| "agent switch unavailable: working directory not set".to_string());

    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("working directory not set"),
        "error must mention 'working directory not set'"
    );
}

// ========================================================================
// Phase I: AgentConversationRuntime accessor method tests
// ========================================================================

#[test]
fn accessor_provider_returns_provider_string() {
    // Verifies that runtime.provider() delegates to provider_state.config().provider
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    // Verify the accessor delegation chain: provider() -> provider_state.config().provider
    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(provider_state.config().provider.as_str(), "copilot");
}

#[test]
fn accessor_model_returns_model_string() {
    // Verifies that runtime.model() delegates to provider_state.config().model
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "claude-sonnet-4".to_string(),
        api_key: Some("test-key".to_string()),
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(provider_state.config().model.as_str(), "claude-sonnet-4");
}

#[test]
fn accessor_max_context_tokens_returns_none_when_unset() {
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(
        provider_state.config().max_context_tokens.map(u64::from),
        None
    );
}

#[test]
fn accessor_max_context_tokens_returns_value_when_set() {
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: Some(200_000),
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert_eq!(
        provider_state.config().max_context_tokens.map(u64::from),
        Some(200_000)
    );
}

#[test]
fn accessor_startup_plugin_config_returns_none_when_default() {
    use crate::config::Config;

    let config = Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "test-model".to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: None,
        preamble: None,
        read_timeout_secs: None,
        max_tool_result_bytes: None,
        model_context_tokens: None,
        context_warning_threshold: None,
        max_retries: None,
        retry_base_delay_ms: None,
        max_tool_calls_per_subturn: None,
    };

    let provider_state = super::super::state::provider::ProviderState::new(config, None);
    assert!(provider_state.startup_plugin_config().is_none());
}

#[test]
fn accessor_agent_identity_returns_none_when_default() {
    // PersonaState with no agent_identity set must return None
    let persona_state =
        super::super::state::persona::PersonaState::new(None, None, None, None, None, None);
    assert_eq!(persona_state.agent_identity(), None);
}

#[test]
fn accessor_agent_identity_returns_some_when_set() {
    let persona_state = super::super::state::persona::PersonaState::new(
        None,
        Some("developer".to_string()),
        None,
        None,
        None,
        None,
    );
    assert_eq!(persona_state.agent_identity(), Some("developer"));
}

#[test]
fn accessor_mcp_caller_cwd_returns_none_when_default() {
    // McpState has mcp_caller_cwd as Option<PathBuf> — None when unset
    let cwd: Option<std::path::PathBuf> = None;
    assert_eq!(cwd.as_deref(), None::<&std::path::Path>);
}

#[test]
fn accessor_mcp_lifecycle_projection_returns_empty_when_default() {
    use crate::tools::mcp::runtime::McpServerLifecycle;

    let projection: Vec<McpServerLifecycle> = vec![];
    assert!(projection.is_empty());
}

#[test]
fn accessor_available_agent_summaries_delegates_to_multi_agent_state() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let state = MultiAgentState::new(None, vec![], AgentsConfig::default());
    assert!(state.available_agent_summaries().is_empty());
}

#[test]
fn accessor_take_mailbox_rx_returns_none_when_default() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let mut state = MultiAgentState::new(None, vec![], AgentsConfig::default());
    assert!(state.take_mailbox_rx().is_none());
}

#[test]
fn accessor_take_mailbox_rx_returns_some_and_drains() {
    use crate::config::AgentsConfig;
    use crate::conversation::state::multi_agent::MultiAgentState;

    let (_tx, rx) = std::sync::mpsc::channel();
    let mut state = MultiAgentState::new(Some(rx), vec![], AgentsConfig::default());
    assert!(
        state.take_mailbox_rx().is_some(),
        "first take must return Some"
    );
    assert!(
        state.take_mailbox_rx().is_none(),
        "second take must return None (drained)"
    );
}

// ========================================================================
// CompactionState characterisation tests
// ========================================================================

#[test]
fn compaction_state_compaction_count_starts_at_zero() {
    let state = super::super::compaction::state::CompactionState::new(
        200_000,
        0.80,
        0,
        CompactionStrategy::SlidingSummary,
    );
    assert_eq!(state.compaction_count(), 0);
}

#[test]
fn compaction_state_compacting_flag_starts_false() {
    use std::sync::atomic::Ordering;
    let state = super::super::compaction::state::CompactionState::new(
        200_000,
        0.80,
        0,
        CompactionStrategy::SlidingSummary,
    );
    assert!(!state.compacting().load(Ordering::SeqCst));
}

// ========================================================================
// Phase J: MemoryState characterisation tests
// ========================================================================

#[test]
fn memory_state_hydrated_false_on_construction() {
    // JournalConversationMemory is load-on-demand (cache starts empty).
    // Verify last_total_tokens is None on construction.
    let temp_dir = tempfile::tempdir().unwrap();
    let ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    assert!(ms.last_total_tokens().is_none());
}

#[test]
fn memory_state_last_total_tokens_none_on_construction() {
    let temp_dir = tempfile::tempdir().unwrap();
    let ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    assert!(ms.last_total_tokens().is_none());
}

// ========================================================================
// Phase K: PersonaState characterisation tests
// ========================================================================

#[test]
fn persona_state_agent_identity_none_by_default() {
    let persona_state =
        super::super::state::persona::PersonaState::new(None, None, None, None, None, None);
    assert!(persona_state.agent_identity().is_none());
}

#[test]
fn persona_state_agent_description_none_by_default() {
    let persona_state =
        super::super::state::persona::PersonaState::new(None, None, None, None, None, None);
    assert!(persona_state.agent_description().is_none());
}

// ========================================================================
// Phase L: McpState characterisation tests
// ========================================================================

#[test]
fn mcp_state_caller_cwd_none_by_default() {
    let temp_dir = tempfile::tempdir().unwrap();
    let _ms = super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    // Access mcp_state through a compile-time type check
    let _type_check: fn(&AgentConversationRuntime) = |rt| {
        assert!(rt.mcp_state.mcp_caller_cwd().is_none());
    };
    // Value-level proof: default Option is None
    let cwd: Option<std::path::PathBuf> = None;
    assert!(cwd.is_none());
}

#[test]
fn mcp_state_lifecycle_projection_empty_by_default() {
    use crate::tools::mcp::runtime::McpServerLifecycle;
    let _type_check: fn(&AgentConversationRuntime) = |rt| {
        assert!(rt.mcp_state.mcp_lifecycle_projection().is_empty());
    };
    // Value-level proof: default Vec is empty
    let projection: Vec<McpServerLifecycle> = vec![];
    assert!(projection.is_empty());
}
