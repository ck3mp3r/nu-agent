//! Integration tests for `build_runtime()`.
//!
//! `build_runtime` constructs real provider clients (an HTTP client plus TLS
//! provider initialisation), so these tests are gated behind the
//! `integration` feature.

use super::*;

use crate::bus::create_bus;
use crate::compaction::CompactionParams;
use crate::config::models_cache::{ModelCost, ModelLimit, ModelSpec, ProviderSpec};
use crate::config::{AgentsConfig, ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig};
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
    plugin_config: Option<PluginConfig>,
    store_type: StoreType,
) -> RuntimeBuildParams {
    RuntimeBuildParams {
        runtime: tokio::runtime::Handle::current(),
        config,
        plugin_config,
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

/// A plugin config whose `[models.light]` role points at an `ollama` cache
/// row carrying a context limit and a price.
fn light_plugin_config() -> PluginConfig {
    let row = ModelSpec {
        id: "light-model".to_string(),
        name: "light-model".to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 262_000,
            output: 8_192,
        },
        cost: Some(ModelCost {
            input: 0.1,
            output: 0.2,
            cache_read: None,
            cache_write: None,
        }),
        modalities: None,
    };
    PluginConfig {
        models: std::collections::HashMap::from([(
            "light".to_string(),
            ModelRoleConfig {
                model: "ollama/light-model".to_string(),
                ..ModelRoleConfig::default()
            },
        )]),
        models_cache: Some(crate::config::ModelsCache {
            providers: std::collections::HashMap::from([(
                "ollama".to_string(),
                ProviderSpec {
                    id: "ollama".to_string(),
                    name: "ollama".to_string(),
                    env: Vec::new(),
                    api: None,
                    models: std::collections::HashMap::from([("light-model".to_string(), row)]),
                },
            )]),
        }),
        ..PluginConfig::default()
    }
}

/// A plugin config whose `[models.light]` role points at a custom-named
/// provider block (`ollama-cloud`) that declares the `ollama` rig impl.
fn custom_named_light_plugin_config() -> PluginConfig {
    let row = ModelSpec {
        id: "light-model".to_string(),
        name: "light-model".to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 262_000,
            output: 8_192,
        },
        cost: Some(ModelCost {
            input: 0.1,
            output: 0.2,
            cache_read: None,
            cache_write: None,
        }),
        modalities: None,
    };
    PluginConfig {
        models: std::collections::HashMap::from([(
            "light".to_string(),
            ModelRoleConfig {
                model: "ollama-cloud/light-model".to_string(),
                ..ModelRoleConfig::default()
            },
        )]),
        providers: std::collections::HashMap::from([(
            "ollama-cloud".to_string(),
            ProviderConfig {
                provider: Some("ollama".to_string()),
                models: std::collections::HashMap::from([(
                    "light-model".to_string(),
                    ModelConfig::default(),
                )]),
                ..ProviderConfig::default()
            },
        )]),
        models_cache: Some(crate::config::ModelsCache {
            providers: std::collections::HashMap::from([(
                "ollama-cloud".to_string(),
                ProviderSpec {
                    id: "ollama-cloud".to_string(),
                    name: "ollama-cloud".to_string(),
                    env: Vec::new(),
                    api: None,
                    models: std::collections::HashMap::from([("light-model".to_string(), row)]),
                },
            )]),
        }),
        ..PluginConfig::default()
    }
}

/// A startup config whose provider is a custom config key declaring the
/// `ollama` rig impl, as `resolve_model` leaves it.
fn custom_named_startup_config() -> Config {
    Config {
        provider: "ollama-cloud".to_string(),
        model: "llama3".to_string(),
        provider_impl: Some("ollama".to_string()),
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    }
}

/// The plugin config declaring the `ollama` impl for the `ollama-cloud` key.
fn custom_named_startup_plugin_config() -> PluginConfig {
    PluginConfig {
        providers: std::collections::HashMap::from([(
            "ollama-cloud".to_string(),
            ProviderConfig {
                provider: Some("ollama".to_string()),
                models: std::collections::HashMap::from([(
                    "llama3".to_string(),
                    ModelConfig::default(),
                )]),
                ..ProviderConfig::default()
            },
        )]),
        ..PluginConfig::default()
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
    let runtime = build_runtime(make_params(cwd.clone(), offline_config(), None, store_type))
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
        None,
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
    let error = build_runtime(make_params(
        temp.path().to_path_buf(),
        config,
        None,
        store_type,
    ))
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

/// With `[models.light]` configured, the compactor is built from the light
/// role: the planning spec describes the light model and the wire carries its
/// price.
#[tokio::test]
async fn test_runtime_build_compaction_model_uses_the_light_role() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        Some(light_plugin_config()),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve")?;
    assert_eq!(
        spec.id, "light-model",
        "the planning spec must describe the light model"
    );
    assert_eq!(
        spec.context_window,
        Some(262_000),
        "the planning spec must carry the light row's context limit"
    );
    let wire = runtime.compaction.model();
    assert_eq!(
        wire.id(),
        Some("light-model"),
        "the compactor's wire must be the light model"
    );
    let pricing = wire
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the light row's price must reach the wire")?;
    assert!(
        (pricing.input - 0.1).abs() < 1e-12 && (pricing.output - 0.2).abs() < 1e-12,
        "the wire must carry the light row's rates, got {} and {}",
        pricing.input,
        pricing.output
    );
    Ok(())
}

/// Without `[models.light]`, the compactor keeps the startup active model.
#[tokio::test]
async fn test_runtime_build_compaction_model_falls_back_to_the_startup_model() -> Result<()> {
    // -- Setup & Fixtures
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        None,
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve")?;
    assert_eq!(
        spec.id, "llama3",
        "without a light role the planning spec must describe the startup model"
    );
    assert_eq!(
        spec.context_window,
        Some(defaults::MAX_CONTEXT_TOKENS),
        "the startup model's default context window must reach the spec"
    );
    assert_eq!(
        runtime.compaction.model().id(),
        Some("llama3"),
        "the compactor's wire must be the startup model"
    );
    Ok(())
}

/// A custom-named provider block that declares the `ollama` rig impl must
/// still resolve the planning spec: the spec names the vendor the wire
/// reports, not the config key.
#[tokio::test]
async fn test_runtime_build_compaction_model_resolves_a_custom_named_light_provider() -> Result<()>
{
    // -- Setup & Fixtures: the light role points at `ollama-cloud`, a config
    // key that is not a rig provider id.
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        offline_config(),
        Some(custom_named_light_plugin_config()),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve for a custom-named provider")?;
    assert_eq!(
        spec.id, "light-model",
        "the planning spec must describe the light model"
    );
    assert_eq!(
        spec.context_window,
        Some(262_000),
        "the planning spec must carry the light row's context limit"
    );
    let wire = runtime.compaction.model();
    assert_eq!(
        wire.id(),
        Some("light-model"),
        "the compactor's wire must be the light model"
    );
    let pricing = wire
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the light row's price must reach the wire")?;
    assert!(
        (pricing.input - 0.1).abs() < 1e-12 && (pricing.output - 0.2).abs() < 1e-12,
        "the wire must carry the light row's rates, got {} and {}",
        pricing.input,
        pricing.output
    );
    Ok(())
}

/// The startup-fallback path resolves the same way: a custom-named startup
/// provider that declares the `ollama` rig impl still yields a planning spec.
#[tokio::test]
async fn test_runtime_build_compaction_model_resolves_a_custom_named_startup_provider() -> Result<()>
{
    // -- Setup & Fixtures: no light role, and the startup provider is the
    // custom config key `ollama-cloud`.
    let temp = tempfile::tempdir()?;
    let store_type = StoreType::Memory;

    // -- Exec
    let runtime = build_runtime(make_params(
        temp.path().to_path_buf(),
        custom_named_startup_config(),
        Some(custom_named_startup_plugin_config()),
        store_type,
    ))
    .await
    .map_err(|e| format!("should build runtime: {e:?}"))?;

    // -- Check
    let spec = runtime
        .compaction
        .spec()
        .ok_or("the planning spec must resolve for a custom-named provider")?;
    assert_eq!(
        spec.id, "llama3",
        "the planning spec must describe the startup model"
    );
    assert_eq!(
        spec.context_window,
        Some(defaults::MAX_CONTEXT_TOKENS),
        "the startup model's default context window must reach the spec"
    );
    assert_eq!(
        runtime.compaction.model().id(),
        Some("llama3"),
        "the compactor's wire must be the startup model"
    );
    Ok(())
}

// endregion: --- build_runtime
