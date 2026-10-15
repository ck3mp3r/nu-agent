use std::path::{Path, PathBuf};
use std::time::Duration;

use nu_protocol::LabeledError;
use rig::http_client::{HttpClientExt, ReqwestClient};
use rig::providers::copilot::auth::{
    AuthError, AuthSource, Authenticator, DeviceCodeHandler, default_token_dir,
};
use rig::providers::copilot::{Copilot, CopilotConfig};

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
/// 4. OAuth — cached credentials, refreshed through rig's `Authenticator`
///
/// Cases 1 and 2 are already exchanged Copilot session keys, so they are sent
/// as-is. Cases 3 and 4 resolve through rig's `Authenticator`: a GitHub access
/// token is exchanged for a session key, and a cached session key is reused.
/// The device flow stays disabled at runtime, so a cold cache reports that
/// sign-in is required instead of blocking a turn for up to 15 minutes.
pub(super) async fn build_copilot_client(config: &Config) -> Result<Copilot, LabeledError> {
    build_copilot_client_with(config, &process_env(), None).await
}

/// Build a GitHub Copilot client against an explicit environment map.
///
/// `token_dir` overrides the credential directory. When it is absent, rig's
/// platform default (`$XDG_CONFIG_HOME/github_copilot`) is used.
pub(super) async fn build_copilot_client_with(
    config: &Config,
    env: &EnvMap,
    token_dir: Option<&Path>,
) -> Result<Copilot, LabeledError> {
    let http_client = build_http_client(config.read_timeout_secs)?;
    build_copilot_client_on(
        config,
        env,
        resolve_token_dir(token_dir),
        ReqwestClient::from(http_client),
    )
    .await
}

/// Resolve the credential directory: an explicit directory wins, otherwise
/// rig's platform default. `None` means no directory can hold credentials.
fn resolve_token_dir(token_dir: Option<&Path>) -> Option<PathBuf> {
    token_dir.map(Path::to_path_buf).or_else(default_token_dir)
}

/// Build a GitHub Copilot client on an injected transport.
///
/// `token_dir` is the already-resolved credential directory. The transport is
/// generic so tests drive credential resolution without a network.
async fn build_copilot_client_on<H>(
    config: &Config,
    env: &EnvMap,
    token_dir: Option<PathBuf>,
    http: H,
) -> Result<Copilot, LabeledError>
where
    H: HttpClientExt + 'static,
{
    // Base URL: config takes precedence, then env vars (same as rig's from_env)
    let base_url = config
        .base_url
        .clone()
        .or_else(|| non_empty(env, "GITHUB_COPILOT_API_BASE"))
        .or_else(|| non_empty(env, "COPILOT_BASE_URL"));

    // 1. Explicit api_key from --api-key flag or plugin config
    // 2. GITHUB_COPILOT_API_KEY / COPILOT_API_KEY env var
    let static_key = config
        .api_key
        .clone()
        .or_else(|| non_empty(env, "GITHUB_COPILOT_API_KEY"))
        .or_else(|| non_empty(env, "COPILOT_API_KEY"));

    // 3. COPILOT_GITHUB_ACCESS_TOKEN / GITHUB_TOKEN env var
    // 4. OAuth — cached credentials, refreshed through rig's `Authenticator`
    let source = if static_key.is_some() {
        None
    } else {
        Some(
            non_empty(env, "COPILOT_GITHUB_ACCESS_TOKEN")
                .or_else(|| non_empty(env, "GITHUB_TOKEN"))
                .map_or(AuthSource::OAuth, AuthSource::GitHubAccessToken),
        )
    };

    let mut copilot_config = CopilotConfig::new(static_key.unwrap_or_default());
    if let Some(url) = &base_url {
        copilot_config = copilot_config.with_base_url(url.clone());
    }

    let Some(source) = source else {
        log::debug!("Copilot auth: using a configured Copilot API key");
        return Ok(copilot_config.connect(http));
    };

    let token_dir = token_dir.ok_or_else(|| {
        LabeledError::new(
            "GitHub Copilot credential directory unavailable: set XDG_CONFIG_HOME or HOME, \
             or pass an explicit token directory.",
        )
    })?;

    log::debug!("Copilot auth: resolving {source:?} through the Authenticator");
    let authenticator = Authenticator::new(
        source,
        Some(token_dir.join("access-token")),
        Some(token_dir.join("api-key.json")),
        DeviceCodeHandler::default(),
        false,
    );

    copilot_config
        .connect(http)
        .authenticate(&authenticator)
        .await
        .map_err(copilot_auth_error)
}

/// Map a rig auth failure to a labeled error, naming the sign-in command when
/// the failure is a missing credential.
fn copilot_auth_error(error: AuthError) -> LabeledError {
    let message = error.to_string();
    if message.contains("sign-in required") {
        return LabeledError::new(format!(
            "{message} Run `agent provider auth login github-copilot` to sign in."
        ));
    }
    LabeledError::new(format!("GitHub Copilot authentication failed: {message}"))
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

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/conversation/providers_integration.rs"]
mod providers_integration_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/conversation/providers_credentials.rs"]
mod providers_credentials_test;
