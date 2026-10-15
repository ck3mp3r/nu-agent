//! `ModelSwitching` and `McpManagement` tests driven through the real
//! `AgentConversationRuntime`.

use super::harness::*;
use super::*;
use crate::protocol::contracts::McpUsabilityState;
use crate::protocol::mcp_management::McpManagement;
use crate::protocol::model_switching::ModelSwitching;
use crate::tools::authz::{PermissionAction, PermissionDecision, PermissionRuleMatch};
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::client::McpToolDefinition;

/// A persona file that exercises every front-matter field `switch_agent`
/// applies to the runtime.
const REVIEWER_PERSONA: &str = r#"---
name: reviewer
description: Reviews code changes
icon: magnifier
model: openai/gpt-4o
temperature: 0.3
max_tokens: 2048
max_tool_turns: 7
max_tool_calls_per_subturn: 3
max_tool_result_bytes: 1234
permissions:
  "*": "allow"
---
You are the reviewer.
"#;

/// A persona file with no front matter at all.
const PLAIN_PERSONA: &str = "You are a plain agent.\n";

fn allow_decision() -> PermissionDecision {
    PermissionDecision {
        action: PermissionAction::Allow,
        matched_rule: PermissionRuleMatch {
            identity: "test-rule".to_string(),
            scope: "tool",
            target_field: None,
            pattern: "*".to_string(),
            action: PermissionAction::Allow,
        },
        diagnostics: Vec::new(),
    }
}

fn tool_definition(name: &str) -> Result<ToolDefinition> {
    Ok(ToolDefinition {
        name: ToolName::new(name)?,
        description: format!("{name} tool"),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    })
}

#[tokio::test]
async fn switch_model_without_a_startup_plugin_config_reports_unavailable() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused")).expect("fixture");

    // -- Exec
    let error = fixture
        .runtime
        .switch_model("openai/gpt-4o")
        .await
        .expect_err("a missing startup plugin config must fail the switch");

    // -- Check
    assert!(
        error.contains("model switch unavailable"),
        "the error must name the missing cache; got: {error}"
    );
}

#[tokio::test]
async fn switch_agent_without_a_caller_cwd_reports_unavailable() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime_without_caller_cwd(text_model("unused")).expect("fixture");

    // -- Exec
    let error = fixture
        .runtime
        .switch_agent("reviewer")
        .await
        .expect_err("a missing caller cwd must fail the switch");

    // -- Check
    assert!(
        error.contains("working directory not set"),
        "the error must name the missing working directory; got: {error}"
    );
}

#[tokio::test]
async fn switch_agent_reports_an_unknown_persona() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused")).expect("fixture");

    // -- Exec
    let error = fixture
        .runtime
        .switch_agent("does-not-exist")
        .await
        .expect_err("an unknown persona must fail the switch");

    // -- Check
    assert!(
        error.contains("agent switch failed"),
        "the error must name the failed switch; got: {error}"
    );
}

#[tokio::test]
async fn switch_agent_applies_persona_model_config_and_permissions() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.write_persona("reviewer", REVIEWER_PERSONA)?;

    // -- Exec
    let identity = fixture
        .runtime
        .switch_agent("reviewer")
        .await
        .map_err(|e| format!("switch_agent should succeed: {e}"))?;

    // -- Check
    assert_eq!(identity, "reviewer");
    assert_eq!(fixture.runtime.persona.agent_identity(), Some("reviewer"));
    assert_eq!(
        fixture.runtime.persona.agent_description(),
        Some("Reviews code changes")
    );
    assert_eq!(fixture.runtime.persona.agent_icon(), Some("magnifier"));
    assert_eq!(
        fixture.runtime.persona.agent_persona_body().map(str::trim),
        Some("You are the reviewer.")
    );

    let config = fixture.runtime.provider.provider_config();
    assert_eq!(config.temperature, Some(0.3));
    assert_eq!(config.max_tokens, Some(2048));
    assert_eq!(config.max_tool_turns, Some(7));
    assert_eq!(config.max_tool_calls_per_subturn, Some(3));
    assert_eq!(config.max_tool_result_bytes, Some(1234));

    assert_eq!(
        fixture
            .runtime
            .permission_state
            .permissions()
            .summary()
            .global,
        PermissionAction::Allow,
        "the persona permissions overlay must replace the base policy"
    );
    assert!(
        fixture.runtime.provider.cached_client().is_none(),
        "the cached client must be invalidated so the next turn rebuilds it"
    );
    Ok(())
}

#[tokio::test]
async fn switch_agent_resets_session_grants_and_tool_definitions() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.write_persona("plain", PLAIN_PERSONA)?;
    fixture
        .runtime
        .tools
        .tool_definitions_mut()
        .push(tool_definition("extra_tool")?);

    let decision = allow_decision();
    let args = serde_json::Map::new();
    let grants = fixture.runtime.permission_state.session_grants_arc();
    grants
        .lock()
        .expect("grants lock")
        .insert_allow_always(&decision, "read", "test", &args);
    assert!(
        grants
            .lock()
            .expect("grants lock")
            .get(&decision, "read", "test", &args)
            .is_some(),
        "the fixture must start with a live session grant"
    );

    // -- Exec
    fixture
        .runtime
        .switch_agent("plain")
        .await
        .map_err(|e| format!("switch_agent should succeed: {e}"))?;

    // -- Check
    assert!(
        grants
            .lock()
            .expect("grants lock")
            .get(&decision, "read", "test", &args)
            .is_none(),
        "an agent switch must drop the previous agent's session grants"
    );
    assert_eq!(
        fixture
            .runtime
            .tools
            .active_definitions(
                fixture.runtime.mcp_state.mcp_registry(),
                fixture.runtime.permission_state.permissions(),
            )
            .len(),
        0,
        "an agent switch must reset tool definitions to the baseline"
    );
    Ok(())
}

#[tokio::test]
async fn switch_agent_without_a_permissions_block_clears_the_overlay() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.write_persona("reviewer", REVIEWER_PERSONA)?;
    fixture.write_persona("plain", PLAIN_PERSONA)?;
    fixture
        .runtime
        .switch_agent("reviewer")
        .await
        .map_err(|e| format!("first switch should succeed: {e}"))?;
    assert_eq!(
        fixture
            .runtime
            .permission_state
            .permissions()
            .summary()
            .global,
        PermissionAction::Allow
    );

    // -- Exec
    fixture
        .runtime
        .switch_agent("plain")
        .await
        .map_err(|e| format!("second switch should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        fixture
            .runtime
            .permission_state
            .permissions()
            .summary()
            .global,
        PermissionAction::Ask,
        "a persona without a permissions block must restore the base policy"
    );
    Ok(())
}

#[tokio::test]
async fn model_switching_accessors_read_the_provider_config() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused")).expect("fixture");
    fixture
        .runtime
        .provider
        .provider_config_mut()
        .max_context_tokens = Some(123_456);

    // -- Exec & Check
    assert_eq!(fixture.runtime.active_model_identity(), "mock/mock-model");
    assert_eq!(fixture.runtime.max_context_tokens(), Some(123_456));
    assert_eq!(
        fixture.runtime.agent_description(),
        Some("test description")
    );
    assert_eq!(fixture.runtime.agent_icon(), Some("robot"));
}

#[tokio::test]
async fn mcp_management_counts_visible_tools_per_server() -> Result<()> {
    // -- Setup & Fixtures
    let registry = McpToolRegistry::from_tools([
        McpToolDefinition {
            server: "alpha".to_string(),
            name: "alpha__one".to_string(),
            raw_name: "one".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "alpha".to_string(),
            name: "alpha__two".to_string(),
            raw_name: "two".to_string(),
            description: None,
            parameters: None,
        },
        McpToolDefinition {
            server: "beta".to_string(),
            name: "beta__three".to_string(),
            raw_name: "three".to_string(),
            description: None,
            parameters: None,
        },
    ])
    .map_err(|e| format!("registry should build: {e}"))?;
    let definitions = vec![
        tool_definition("alpha__one")?,
        tool_definition("alpha__two")?,
        tool_definition("beta__three")?,
    ];
    let fixture = make_runtime_with_mcp(text_model("unused"), registry, definitions)?;

    // -- Exec & Check
    assert_eq!(fixture.runtime.llm_visible_mcp_tool_count(), 3);
    assert_eq!(
        fixture
            .runtime
            .llm_visible_mcp_tool_count_for_server("alpha"),
        2
    );
    assert_eq!(
        fixture
            .runtime
            .llm_visible_mcp_tool_count_for_server("beta"),
        1
    );
    assert_eq!(
        fixture
            .runtime
            .llm_visible_mcp_tool_count_for_server("gamma"),
        0
    );
    assert_eq!(
        fixture.runtime.llm_visible_mcp_tool_names_by_server(),
        vec![
            (
                "alpha".to_string(),
                vec!["alpha__one".to_string(), "alpha__two".to_string()]
            ),
            ("beta".to_string(), vec!["beta__three".to_string()]),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn mcp_management_disabling_a_server_reports_disabled() -> Result<()> {
    // -- Setup & Fixtures
    let registry = McpToolRegistry::from_tools([McpToolDefinition {
        server: "alpha".to_string(),
        name: "alpha__one".to_string(),
        raw_name: "one".to_string(),
        description: None,
        parameters: None,
    }])
    .map_err(|e| format!("registry should build: {e}"))?;
    let mut fixture = make_runtime_with_mcp(
        text_model("unused"),
        registry,
        vec![tool_definition("alpha__one")?],
    )?;

    // -- Exec
    let state = fixture
        .runtime
        .set_mcp_server_enabled("alpha", false)
        .await
        .map_err(|e| format!("disabling a known server should succeed: {e}"))?;

    // -- Check
    assert_eq!(state, McpUsabilityState::Disabled);
    assert_eq!(
        fixture.runtime.llm_visible_mcp_tool_count(),
        0,
        "a disabled server's tools must leave the LLM-visible set"
    );
    Ok(())
}

#[tokio::test]
async fn mcp_management_enabling_an_unconfigured_server_reports_failed() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;

    // -- Exec
    let state = fixture
        .runtime
        .set_mcp_server_enabled("not-configured", true)
        .await
        .map_err(|e| format!("enabling an unconfigured server must not error: {e}"))?;

    // -- Check
    assert_eq!(state, McpUsabilityState::Failed);
    Ok(())
}
