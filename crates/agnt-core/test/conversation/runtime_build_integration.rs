//! Integration tests for `build_runtime()`.
//!
//! `build_runtime` constructs real provider clients (an HTTP client plus TLS
//! provider initialisation), so these tests are gated behind the
//! `integration` feature.

use super::*;

use crate::bus::create_bus;
use crate::compaction::CompactionParams;
use crate::config::AgentsConfig;
use crate::session::StoreType;
use crate::tools::authz::PermissionsConfig;
use crate::tools::closure::{ClosureRegistry, CwdEngine};
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A config whose provider builds offline: `ollama` needs only a base URL.
fn offline_config() -> Config {
    Config {
        provider: "ollama".to_string(),
        model: "llama3".to_string(),
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    }
}

fn make_params(
    cwd: std::path::PathBuf,
    config: Config,
    store_type: StoreType,
) -> RuntimeBuildParams {
    RuntimeBuildParams {
        runtime: tokio::runtime::Handle::current(),
        config,
        plugin_config: None,
        tool_definitions: Vec::new(),
        baseline_tool_definitions: Vec::new(),
        closure_registry: ClosureRegistry::default(),
        mcp_runtime: None,
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        mcp_lifecycle_projection: Vec::new(),
        mcp_server_configs: Vec::new(),
        mcp_caller_cwd: None,
        vault: None,
        mcp_registry: McpToolRegistry::empty(),
        engine: CwdEngine::shared(cwd.to_string_lossy().to_string()),
        store_type,
        final_session_id: None,
        compaction_params: CompactionParams::default(),
        proactive_threshold_pct: None,
        base_permissions: PermissionsConfig::safe_defaults(true),
        effective_permissions: PermissionsConfig::safe_defaults(true),
        cli_permissions_overlay: None,
        permissions_startup_summary: "test permissions summary".to_string(),
        persona_body: Some("persona body".to_string()),
        agent_identity: Some("test-agent".to_string()),
        agent_description: Some("test description".to_string()),
        agent_icon: Some("robot".to_string()),
        cached_agents_chain: Some("agents chain".to_string()),
        cached_available_skills: Some("available skills".to_string()),
        cached_sub_agent_instruction: None,
        available_agents: Vec::new(),
        agents_config: AgentsConfig::default(),
        cwd,
        bus: create_bus(),
        render_registry: ToolRenderRegistry::default(),
    }
}

// endregion: --- Test Support

// region:    --- build_runtime

#[tokio::test]
async fn test_runtime_build_build_runtime_constructs_runtime_from_params() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("session-cwd");
    std::fs::create_dir_all(&cwd)?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(cwd.clone(), offline_config(), store_type))
        .await
        .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(runtime.cwd, cwd, "runtime cwd must come from params");
    assert!(
        runtime.final_session_id.is_none(),
        "a fresh session has no persisted session id"
    );
    assert_eq!(runtime.provider.config().provider, "ollama");
    assert_eq!(runtime.provider.config().model, "llama3");
    assert_eq!(runtime.persona.agent_identity(), Some("test-agent"));
    assert_eq!(runtime.persona.agent_persona_body(), Some("persona body"));
    assert_eq!(runtime.persona.cached_agents_chain(), Some("agents chain"));
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_build_runtime_uses_default_compaction_threshold() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;
    let expected =
        (defaults::MAX_CONTEXT_TOKENS as f64 * defaults::PROACTIVE_THRESHOLD_PCT) as usize;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    assert_eq!(runtime.compaction.threshold_tokens, Some(expected));
    Ok(())
}

#[tokio::test]
async fn test_runtime_build_build_runtime_unsupported_provider_reports_model_error() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;
    let config = Config {
        provider: "not-a-provider".to_string(),
        model: "whatever".to_string(),
        ..Config::default()
    };

    // -- Exec
    let error = build_runtime(make_params(temp.path().to_path_buf(), config, store_type))
        .await
        .err()
        .ok_or("an unsupported provider must fail")?;

    // -- Check
    assert!(
        error.to_string().contains("Failed to build model"),
        "unexpected error: {error}"
    );
    Ok(())
}

// endregion: --- build_runtime
