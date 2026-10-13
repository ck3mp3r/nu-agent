use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

use std::path::Path;

use rig::completion::CompletionRequest;
use rig::test_utils::RecordingHttpClient;
use rig::wire::{Mode, Wire};

use crate::config::Config;
use crate::utils::env_map::EnvMap;

#[test]
fn resolve_provider_type_uses_explicit_field() {
    assert_eq!(
        super::resolve_provider_type("ollama-remote", Some("ollama")),
        "ollama"
    );
}

#[test]
fn resolve_provider_type_falls_back_to_key() {
    assert_eq!(super::resolve_provider_type("ollama", None), "ollama");
}

#[test]
fn resolve_provider_type_custom_key_with_known_impl() {
    assert_eq!(
        super::resolve_provider_type("my-openai", Some("openai")),
        "openai"
    );
}

// ========================================================================
// Phase 1b: CachedProviderClient, resolve_provider_type, ClientCacheKey
// ========================================================================

#[test]
fn client_cache_key_type_is_four_tuple() {
    let key: ClientCacheKey = ("copilot".to_string(), None, None, None);
    assert_eq!(key.0, "copilot");
    assert_eq!(key.1, None);
    assert_eq!(key.2, None);
    assert_eq!(key.3, None);
}

#[test]
fn resolve_provider_type_uses_field_when_set() {
    assert_eq!(
        super::resolve_provider_type("my-provider", Some("copilot")),
        "copilot"
    );
}

#[test]
fn resolve_provider_type_falls_back_to_key_when_field_none() {
    assert_eq!(
        super::resolve_provider_type("github-copilot", None),
        "github-copilot"
    );
}

// ========================================================================
// read_timeout_secs pass-through tests
// ========================================================================

#[test]
fn plugin_config_read_timeout_secs_propagates_to_resolved_config() -> Result<()> {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: Some("sk-test".to_string()),
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );

    let plugin_config = PluginConfig {
        models: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    read_timeout_secs: Some(60),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        read_timeout_secs: Some(60),
        ..ModelRoleConfig::default()
    };
    let resolved = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(
        resolved.read_timeout_secs,
        Some(60),
        "read_timeout_secs should be 60 after resolve"
    );
    Ok(())
}

#[test]
fn plugin_config_without_read_timeout_secs_resolves_to_none() -> Result<()> {
    use std::collections::HashMap;

    use crate::config::{AgentsConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: None,
            api_key: Some("sk-test".to_string()),
            base_url: None,
            provider: None,
            preamble: None,
            models: HashMap::new(),
        },
    );

    let plugin_config = PluginConfig {
        models: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers,
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };
    let resolved = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(
        resolved.read_timeout_secs, None,
        "read_timeout_secs should be None when not configured"
    );
    Ok(())
}

// ========================================================================
// Copilot credential resolution through rig's Authenticator
// ========================================================================

/// The exchanged Copilot session key the scripted GitHub exchange returns.
const EXCHANGED_KEY: &str = "tid=exchanged-session";

/// The raw GitHub access token the environment supplies.
const GITHUB_TOKEN: &str = "gho_raw-github-token";

/// A Copilot config with no credential in config or environment.
fn copilot_config() -> Config {
    Config {
        provider: "copilot".to_string(),
        model: "gpt-4o".to_string(),
        ..Config::default()
    }
}

/// The GitHub token-exchange reply, carrying a live session key.
fn exchange_body() -> String {
    serde_json::json!({
        "token": EXCHANGED_KEY,
        "expires_at": i64::MAX,
        "endpoints": { "api": "https://api.enterprise.githubcopilot.com" },
    })
    .to_string()
}

/// Write a live cached session key into `dir`, as a completed login would.
fn seed_cached_key(dir: &Path) -> Result<()> {
    std::fs::write(
        dir.join("api-key.json"),
        serde_json::json!({ "token": EXCHANGED_KEY, "expires_at": i64::MAX }).to_string(),
    )?;
    Ok(())
}

/// The Authorization header of the first recorded request.
fn recorded_authorization(http: &RecordingHttpClient) -> Option<String> {
    http.requests()
        .first()
        .and_then(|request| request.headers.get(http::header::AUTHORIZATION))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// The Authorization header a completion request would carry.
fn completion_authorization(client: &Copilot) -> Result<String> {
    let model = client.completion("gpt-4o");
    let encoded = model
        .wire
        .encode(CompletionRequest::new("say hi"), Mode::Unary)
        .map_err(|e| format!("should encode: {e:?}"))?;
    encoded
        .request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "a completion request should carry an Authorization header".into())
}

#[tokio::test]
async fn copilot_config_api_key_skips_the_authenticator() -> Result<()> {
    // -- Setup & Fixtures
    let http = RecordingHttpClient::new("");
    let config = Config {
        api_key: Some("tid=static-copilot-key".to_string()),
        ..copilot_config()
    };

    // -- Exec
    // No credential directory resolves: the guard only applies to the
    // Authenticator path, so a static key must still build.
    let client =
        super::build_copilot_client_on(&config, &EnvMap::new(), None, http.clone()).await?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), "tid=static-copilot-key");
    assert_eq!(
        completion_authorization(&client)?,
        "Bearer tid=static-copilot-key"
    );
    assert!(
        http.requests().is_empty(),
        "a configured Copilot key needs no auth request"
    );
    Ok(())
}

#[tokio::test]
async fn copilot_env_api_key_skips_the_authenticator() -> Result<()> {
    // -- Setup & Fixtures
    let http = RecordingHttpClient::new("");
    let env = EnvMap::from([(
        "GITHUB_COPILOT_API_KEY".to_string(),
        "tid=env-copilot-key".to_string(),
    )]);

    // -- Exec
    let client =
        super::build_copilot_client_on(&copilot_config(), &env, None, http.clone()).await?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), "tid=env-copilot-key");
    assert_eq!(
        completion_authorization(&client)?,
        "Bearer tid=env-copilot-key"
    );
    assert!(
        http.requests().is_empty(),
        "a configured Copilot key needs no auth request"
    );
    Ok(())
}

#[tokio::test]
async fn copilot_github_access_token_is_exchanged_for_a_session_key() -> Result<()> {
    // -- Setup & Fixtures
    let token_dir = tempfile::TempDir::new()?;
    let http = RecordingHttpClient::new(exchange_body());
    let env = EnvMap::from([(
        "COPILOT_GITHUB_ACCESS_TOKEN".to_string(),
        GITHUB_TOKEN.to_string(),
    )]);

    // -- Exec
    let client = super::build_copilot_client_on(
        &copilot_config(),
        &env,
        Some(token_dir.path().to_path_buf()),
        http.clone(),
    )
    .await?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), EXCHANGED_KEY);
    let requests = http.requests();
    assert_eq!(requests.len(), 1, "exactly one exchange request");
    assert_eq!(
        requests.first().map(|request| request.uri.as_str()),
        Some("https://api.github.com/copilot_internal/v2/token")
    );
    let expected_exchange = format!("token {GITHUB_TOKEN}");
    assert_eq!(
        recorded_authorization(&http).as_deref(),
        Some(expected_exchange.as_str())
    );
    let completion = completion_authorization(&client)?;
    assert_eq!(completion, format!("Bearer {EXCHANGED_KEY}"));
    assert!(
        !completion.contains(GITHUB_TOKEN),
        "the raw GitHub token must not reach a completion request: {completion}"
    );
    Ok(())
}

#[tokio::test]
async fn copilot_cached_session_key_is_reused_without_a_request() -> Result<()> {
    // -- Setup & Fixtures
    let token_dir = tempfile::TempDir::new()?;
    seed_cached_key(token_dir.path())?;
    let http = RecordingHttpClient::new("");

    // -- Exec
    let client = super::build_copilot_client_on(
        &copilot_config(),
        &EnvMap::new(),
        Some(token_dir.path().to_path_buf()),
        http.clone(),
    )
    .await?;

    // -- Check
    assert_eq!(client.config().api_key.expose(), EXCHANGED_KEY);
    assert!(
        http.requests().is_empty(),
        "a live cached session key must not exchange"
    );
    Ok(())
}

#[tokio::test]
async fn copilot_without_a_credential_reports_sign_in_required() -> Result<()> {
    // -- Setup & Fixtures
    let token_dir = tempfile::TempDir::new()?;
    let http = RecordingHttpClient::new("");

    // -- Exec
    let error = super::build_copilot_client_on(
        &copilot_config(),
        &EnvMap::new(),
        Some(token_dir.path().to_path_buf()),
        http.clone(),
    )
    .await
    .err()
    .ok_or("a cold cache must not build a client")?;

    // -- Check
    assert!(
        error
            .to_string()
            .contains("agent provider auth login github-copilot"),
        "{error}"
    );
    assert!(
        http.requests().is_empty(),
        "the device flow stays disabled at runtime"
    );
    Ok(())
}

#[tokio::test]
async fn copilot_without_a_token_dir_fails_before_any_auth_request() -> Result<()> {
    // -- Setup & Fixtures
    let http = RecordingHttpClient::new("");

    // -- Exec
    let error =
        super::build_copilot_client_on(&copilot_config(), &EnvMap::new(), None, http.clone())
            .await
            .err()
            .ok_or("no credential directory must not build a client")?;

    // -- Check
    assert!(
        error.to_string().contains("credential directory"),
        "{error}"
    );
    assert!(
        http.requests().is_empty(),
        "no auth request without a credential directory"
    );
    Ok(())
}

#[tokio::test]
async fn copilot_oauth_keeps_an_explicit_base_url() -> Result<()> {
    // -- Setup & Fixtures
    let token_dir = tempfile::TempDir::new()?;
    seed_cached_key(token_dir.path())?;
    let http = RecordingHttpClient::new("");
    let config = Config {
        base_url: Some("http://localhost:4141".to_string()),
        ..copilot_config()
    };

    // -- Exec
    let client = super::build_copilot_client_on(
        &config,
        &EnvMap::new(),
        Some(token_dir.path().to_path_buf()),
        http.clone(),
    )
    .await?;

    // -- Check
    assert_eq!(client.config().base_url, "http://localhost:4141");
    assert_eq!(client.config().api_key.expose(), EXCHANGED_KEY);
    Ok(())
}
