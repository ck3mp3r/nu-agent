use std::time::Duration;

use nu_protocol::LabeledError;
use rig::http_client::ReqwestClient;

use crate::config::{Config, defaults};
use crate::utils::crypto::ensure_crypto_provider;
use crate::utils::env_map::{EnvMap, process_env};

/// Build a shared HTTP client with a connect timeout and optional read timeout.
///
/// Read timeout: defaults to 120s — fires only when no bytes received for the duration.
///   Pass `Some(0)` to disable. This is safe for long active LLM responses.
/// Uses system certificate store via rustls-native-certs (supports corporate CAs).
pub(crate) fn build_http_client(
    read_timeout_secs: Option<u64>,
) -> Result<reqwest::Client, LabeledError> {
    ensure_crypto_provider();
    let read_timeout = read_timeout_secs.unwrap_or(defaults::READ_TIMEOUT_SECS);
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Some(Duration::from_secs(55))) // evict before server-side idle timeout (~60-90s)
        .pool_max_idle_per_host(5);
    if read_timeout > 0 {
        builder = builder.read_timeout(Duration::from_secs(read_timeout));
    }
    builder
        .build()
        .map_err(|e| LabeledError::new(format!("Failed to build HTTP client: {e}")))
}

/// Build a GitHub Copilot client, resolving credentials in priority order:
///
/// 1. Explicit `api_key` from plugin config or `--api-key` flag
/// 2. `GITHUB_COPILOT_API_KEY` / `COPILOT_API_KEY` environment variable
/// 3. `COPILOT_GITHUB_ACCESS_TOKEN` / `GITHUB_TOKEN` environment variable
/// 4. OAuth — no credential resolved here; rig-core's `Authenticator` owns the
///    device-code login and token exchange (see task 6793a293).
pub(super) fn build_copilot_client(
    config: &Config,
) -> Result<rig::providers::copilot::Copilot, LabeledError> {
    build_copilot_client_with(config, &process_env(), None)
}

/// Build a GitHub Copilot client against an explicit environment map.
///
/// `token_dir` is accepted for call-site compatibility; rig 0.43 resolves
/// cached credentials through `Authenticator` file paths instead (task 6793a293).
pub(super) fn build_copilot_client_with(
    config: &Config,
    env: &EnvMap,
    token_dir: Option<&std::path::Path>,
) -> Result<rig::providers::copilot::Copilot, LabeledError> {
    let _ = token_dir;

    let http_client = build_http_client(config.read_timeout_secs)?;

    // Base URL: config takes precedence, then env vars (same as rig's from_env)
    let base_url = config
        .base_url
        .clone()
        .or_else(|| non_empty(env, "GITHUB_COPILOT_API_BASE"))
        .or_else(|| non_empty(env, "COPILOT_BASE_URL"));

    // 1. Explicit api_key from --api-key flag or plugin config
    let credential = if let Some(key) = &config.api_key {
        log::debug!("Copilot auth: using explicit api_key");
        Some(key.clone())
    } else if let Some(key) =
        non_empty(env, "GITHUB_COPILOT_API_KEY").or_else(|| non_empty(env, "COPILOT_API_KEY"))
    {
        // 2. GITHUB_COPILOT_API_KEY / COPILOT_API_KEY env var
        log::debug!("Copilot auth: using GITHUB_COPILOT_API_KEY");
        Some(key)
    } else if let Some(token) =
        non_empty(env, "COPILOT_GITHUB_ACCESS_TOKEN").or_else(|| non_empty(env, "GITHUB_TOKEN"))
    {
        // 3. COPILOT_GITHUB_ACCESS_TOKEN / GITHUB_TOKEN env var
        log::debug!("Copilot auth: using COPILOT_GITHUB_ACCESS_TOKEN/GITHUB_TOKEN");
        Some(token)
    } else {
        // 4. OAuth — rig 0.43 resolves this through `Authenticator` (task 6793a293).
        log::debug!("Copilot auth: no credential resolved; OAuth deferred to Authenticator");
        None
    };

    let mut copilot_config =
        rig::providers::copilot::CopilotConfig::new(credential.unwrap_or_default());
    if let Some(url) = &base_url {
        copilot_config = copilot_config.with_base_url(url.clone());
    }
    Ok(copilot_config.connect(ReqwestClient::from(http_client)))
}

/// Look up `key`, treating an empty value as absent.
fn non_empty(env: &EnvMap, key: &str) -> Option<String> {
    env.get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Build an OpenAI client using rig 0.43's config-and-connect pattern.
///
/// If config has an explicit `api_key`, uses the config with optional `base_url`.
/// Otherwise, reads `OPENAI_API_KEY` from the environment (also checks `OPENAI_BASE_URL`).
///
/// A `base_url` selects the Chat Completions route (`/chat/completions`), which
/// every OpenAI-compatible gateway serves; without one the Responses route is used.
pub(super) fn build_openai_client(
    config: &Config,
) -> Result<rig::providers::openai::OpenAI, LabeledError> {
    use rig::providers::openai::{OpenAIConfig, Route};

    log::debug!(
        "OpenAI client: api_key={} base_url={:?}",
        config.api_key.is_some(),
        config.base_url
    );

    let http_client = build_http_client(config.read_timeout_secs)?;

    let (key, base_url) = if let Some(key) = &config.api_key {
        (key.clone(), config.base_url.clone())
    } else {
        let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            LabeledError::new(
                "OpenAI client initialization failed: OPENAI_API_KEY not set.".to_string(),
            )
        })?;
        (key, std::env::var("OPENAI_BASE_URL").ok())
    };

    let mut openai_config = OpenAIConfig::new(key);
    if let Some(url) = base_url {
        openai_config = openai_config.with_base_url(url).with_route(Route::Chat);
    }
    Ok(openai_config.connect(ReqwestClient::from(http_client)))
}

/// Build an Anthropic client using rig 0.43's config-and-connect pattern.
///
/// If config has an explicit `api_key`, uses the config with optional `base_url`.
/// Otherwise, reads `ANTHROPIC_API_KEY` from the environment.
pub(super) fn build_anthropic_client(
    config: &Config,
) -> Result<rig::providers::anthropic::Anthropic, LabeledError> {
    use rig::providers::anthropic::AnthropicConfig;

    log::debug!("Anthropic client: api_key={}", config.api_key.is_some());

    let http_client = build_http_client(config.read_timeout_secs)?;

    let (key, base_url) = if let Some(key) = &config.api_key {
        (key.clone(), config.base_url.clone())
    } else {
        let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            LabeledError::new(
                "Anthropic client initialization failed: ANTHROPIC_API_KEY not set.".to_string(),
            )
        })?;
        (key, None)
    };

    let mut anthropic_config = AnthropicConfig::new(key);
    if let Some(url) = base_url {
        anthropic_config = anthropic_config.with_base_url(url);
    }
    Ok(anthropic_config.connect(ReqwestClient::from(http_client)))
}

/// Build an Ollama client.
///
/// `config.api_key` is optional: when set, it is passed to the client and sent
/// as a Bearer auth header for secured deployments (e.g. ollama-cloud); when
/// absent, no auth header is sent. If `config.base_url` is set, uses that URL.
/// Otherwise reads `OLLAMA_API_BASE_URL` from the environment (defaults to
/// `http://localhost:11434`).
pub(super) fn build_ollama_client(
    config: &Config,
) -> Result<rig::providers::ollama::Ollama, LabeledError> {
    use rig::providers::ollama::OllamaConfig;

    let base_url = config.base_url.clone().unwrap_or_else(|| {
        std::env::var("OLLAMA_API_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string())
    });

    log::debug!("Ollama client: base_url={base_url}");

    let http_client = build_http_client(config.read_timeout_secs)?;

    let mut ollama_config = OllamaConfig::new().with_base_url(base_url);
    if let Some(key) = &config.api_key {
        ollama_config = ollama_config.with_api_key(key.clone());
    }
    Ok(ollama_config.connect(ReqwestClient::from(http_client)))
}

/// Resolve which provider implementation to use.
/// If the provider config specifies an explicit `provider` field, use it.
/// Otherwise, the config key name itself is the provider type.
pub(super) fn resolve_provider_type<'a>(
    provider_key: &'a str,
    provider_field: Option<&'a str>,
) -> &'a str {
    provider_field.unwrap_or(provider_key)
}

pub enum CachedProviderClient {
    Copilot(rig::providers::copilot::Copilot),
    OpenAi(rig::providers::openai::OpenAI),
    /// OpenAI-compatible providers that use `/chat/completions` instead of `/responses`.
    /// Automatically selected when `base_url` is set for the `openai` provider.
    /// In rig 0.43 the route lives in the client's `OpenAIConfig` (`Route::Chat`),
    /// so this variant holds the same client type as [`Self::OpenAi`].
    OpenAiCompletions(rig::providers::openai::OpenAI),
    Anthropic(rig::providers::anthropic::Anthropic),
    Ollama(rig::providers::ollama::Ollama),
    Mock(rig::test_utils::MockCompletionModel),
}

impl CachedProviderClient {
    /// Erase the cached provider's completion model into a `DynModel<Completion>`.
    ///
    /// The erased model runs the same driver as the concrete model it was made
    /// from, so a consumer that only sends completion requests does not name the
    /// provider's wire or transport. Each provider's `completion()` returns a
    /// different concrete `Model<W>`, so the match arms erase to the one shared
    /// `DynModel<Completion>` type.
    pub fn build_dyn_model(
        &self,
        model_name: &str,
    ) -> Result<rig::DynModel<rig::operation::Completion>, nu_protocol::LabeledError> {
        log::debug!("build_dyn_model: model={model_name}");
        let model = match self {
            CachedProviderClient::Copilot(c) => c.completion(model_name).erase(),
            CachedProviderClient::OpenAi(c) => c.completion(model_name).erase(),
            CachedProviderClient::OpenAiCompletions(c) => c.completion(model_name).erase(),
            CachedProviderClient::Anthropic(c) => c.completion(model_name).erase(),
            CachedProviderClient::Ollama(c) => c.completion(model_name).erase(),
            CachedProviderClient::Mock(m) => m.clone().erase(),
        };
        Ok(model)
    }
}

pub type ClientCacheKey = (String, Option<String>, Option<String>, Option<u64>);

#[cfg(test)]
#[path = "../../test/conversation/providers.rs"]
mod providers_test;
