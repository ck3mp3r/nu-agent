use std::time::Duration;

use nu_protocol::LabeledError;

use crate::config::Config;

/// Default read timeout for HTTP streaming responses in seconds.
///
/// This fires only when no bytes are received for this duration —
/// it resets on each successful read, so active long-running responses
/// are not affected.
const DEFAULT_READ_TIMEOUT_SECS: u64 = 30;

/// Build a shared HTTP client with a connect timeout and optional read timeout.
///
/// Read timeout: defaults to 30s — fires only when no bytes received for the duration.
///   Pass `Some(0)` to disable. This is safe for long active LLM responses.
/// Uses system certificate store via rustls-native-certs (supports corporate CAs).
fn build_http_client(read_timeout_secs: Option<u64>) -> reqwest::Client {
    let read_timeout = read_timeout_secs.unwrap_or(DEFAULT_READ_TIMEOUT_SECS);
    let mut builder = reqwest::Client::builder();
    if read_timeout > 0 {
        builder = builder.read_timeout(Duration::from_secs(read_timeout));
    }
    builder.build().expect("failed to build HTTP client")
}

/// Build a GitHub Copilot client using rig's from_env() or explicit config.
///
/// If config has an explicit `api_key`, uses the builder pattern with optional `base_url`.
/// Otherwise, delegates to `rig::providers::copilot::Client::from_env()` which handles
/// environment variable resolution (GITHUB_COPILOT_API_KEY → GITHUB_TOKEN → OAuth).
pub(super) fn build_copilot_client(
    config: &Config,
) -> Result<rig::providers::copilot::Client, LabeledError> {
    let auth_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "Copilot auth failed: {e}. Run `agent auth login` to authenticate."
        ))
    };

    // Base URL: config takes precedence, then env vars (same as rig's from_env)
    let base_url = config
        .base_url
        .clone()
        .or_else(|| {
            std::env::var("GITHUB_COPILOT_API_BASE")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            std::env::var("COPILOT_BASE_URL")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });

    // 1. Explicit api_key from --api-key flag or plugin config
    if let Some(key) = &config.api_key {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs));
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b
            .api_key::<rig::providers::copilot::CopilotAuth>(key.clone())
            .build()
            .map_err(auth_err);
    }

    // 2. GITHUB_COPILOT_API_KEY / COPILOT_API_KEY env var
    if let Ok(key) =
        std::env::var("GITHUB_COPILOT_API_KEY").or_else(|_| std::env::var("COPILOT_API_KEY"))
        && !key.trim().is_empty()
    {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs));
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b
            .api_key::<rig::providers::copilot::CopilotAuth>(key)
            .build()
            .map_err(auth_err);
    }

    // 3. COPILOT_GITHUB_ACCESS_TOKEN / GITHUB_TOKEN env var
    if let Ok(token) =
        std::env::var("COPILOT_GITHUB_ACCESS_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN"))
        && !token.trim().is_empty()
    {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs));
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b.github_access_token(token).build().map_err(auth_err);
    }

    // 4. Cached OAuth access token from prior `agent auth login`
    //    rig caches at: <config_dir>/github_copilot/access-token (plain text)
    let token_path = crate::utils::xdg::config_dir()
        .ok()
        .map(|d| d.join("github_copilot").join("access-token"));

    if let Some(path) = &token_path
        && let Ok(token) = std::fs::read_to_string(path)
        && !token.trim().is_empty()
    {
        let mut b = rig::providers::copilot::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs));
        if let Some(url) = &base_url {
            b = b.base_url(url.clone());
        }
        return b
            .github_access_token(token.trim())
            .build()
            .map_err(auth_err);
    }

    // 5. No auth available
    Err(LabeledError::new(
        "Not authenticated. Run `agent auth login` or set GITHUB_COPILOT_API_KEY / GITHUB_TOKEN environment variable.".to_string()
    ))
}

/// Build an OpenAI client using rig's builder pattern.
///
/// If config has an explicit `api_key`, uses the builder with optional `base_url`.
/// Otherwise, reads `OPENAI_API_KEY` from the environment (also checks `OPENAI_BASE_URL`).
pub(super) fn build_openai_client(
    config: &Config,
) -> Result<rig::providers::openai::Client, LabeledError> {
    let map_build_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "OpenAI client initialization failed: {e}. Ensure OPENAI_API_KEY is set."
        ))
    };

    if let Some(key) = &config.api_key {
        let mut builder = rig::providers::openai::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs));
        if let Some(url) = &config.base_url {
            builder = builder.base_url(url.clone());
        }
        builder.api_key(key.clone()).build().map_err(map_build_err)
    } else {
        let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            LabeledError::new(
                "OpenAI client initialization failed: OPENAI_API_KEY not set.".to_string(),
            )
        })?;
        let mut builder = rig::providers::openai::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs))
            .api_key(key);
        if let Ok(url) = std::env::var("OPENAI_BASE_URL") {
            builder = builder.base_url(url);
        }
        builder.build().map_err(map_build_err)
    }
}

/// Build an Anthropic client using rig's builder pattern.
///
/// If config has an explicit `api_key`, uses the builder with optional `base_url`.
/// Otherwise, reads `ANTHROPIC_API_KEY` from the environment.
pub(super) fn build_anthropic_client(
    config: &Config,
) -> Result<rig::providers::anthropic::Client, LabeledError> {
    let map_build_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "Anthropic client initialization failed: {e}. Ensure ANTHROPIC_API_KEY is set."
        ))
    };

    if let Some(key) = &config.api_key {
        let mut builder = rig::providers::anthropic::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs));
        if let Some(url) = &config.base_url {
            builder = builder.base_url(url.clone());
        }
        builder.api_key(key.clone()).build().map_err(map_build_err)
    } else {
        let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            LabeledError::new(
                "Anthropic client initialization failed: ANTHROPIC_API_KEY not set.".to_string(),
            )
        })?;
        rig::providers::anthropic::Client::builder()
            .http_client(build_http_client(config.read_timeout_secs))
            .api_key(key)
            .build()
            .map_err(map_build_err)
    }
}

/// Build an Ollama client.
///
/// Ollama doesn't require an API key. If `config.base_url` is set, uses that URL.
/// Otherwise reads `OLLAMA_API_BASE_URL` from the environment (defaults to
/// `http://localhost:11434`).
pub(super) fn build_ollama_client(
    config: &Config,
) -> Result<rig::providers::ollama::Client, LabeledError> {
    use rig::client::Nothing;

    let map_build_err = |e: rig::http_client::Error| {
        LabeledError::new(format!(
            "Ollama client initialization failed: {e}. Ensure Ollama is running."
        ))
    };

    let base_url = config.base_url.clone().unwrap_or_else(|| {
        std::env::var("OLLAMA_API_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string())
    });

    rig::providers::ollama::Client::builder()
        .http_client(build_http_client(config.read_timeout_secs))
        .base_url(base_url)
        .api_key(Nothing)
        .build()
        .map_err(map_build_err)
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
    Copilot(rig::providers::copilot::Client),
    OpenAi(rig::providers::openai::Client),
    /// OpenAI-compatible providers that use `/chat/completions` instead of `/responses`.
    /// Automatically selected when `base_url` is set for the `openai` provider.
    OpenAiCompletions(rig::providers::openai::CompletionsClient),
    Anthropic(rig::providers::anthropic::Client),
    Ollama(rig::providers::ollama::Client),
    /// Scripted mock model for unit tests — bypasses all HTTP/auth.
    #[cfg(test)]
    Mock(rig::test_utils::MockCompletionModel),
}

/// Visitor pattern for dispatching over cached provider clients.
///
/// Each provider's `completion_model()` returns a different concrete type,
/// so a plain closure can't be generic over all of them. This trait lets
/// callers define a single generic method that the enum dispatches into,
/// replacing the duplicated `with_cached_model!` macro with static dispatch.
pub trait ModelVisitor {
    type Output;
    fn visit<M>(self, model: M) -> Self::Output
    where
        M: rig::completion::CompletionModel + Clone + 'static;
}

impl CachedProviderClient {
    /// Dispatch the visitor over the cached provider's completion model.
    ///
    /// This replaces the `with_cached_model!` macro: each match arm builds the
    /// concrete completion model and passes it to `visitor.visit(model)`, which
    /// is monomorphised per variant — no dynamic dispatch needed.
    pub fn with_model<V: ModelVisitor>(&self, model_name: &str, visitor: V) -> V::Output {
        use rig::client::CompletionClient;
        match self {
            CachedProviderClient::Copilot(c) => visitor.visit(c.completion_model(model_name)),
            CachedProviderClient::OpenAi(c) => visitor.visit(c.completion_model(model_name)),
            CachedProviderClient::OpenAiCompletions(c) => {
                visitor.visit(c.completion_model(model_name))
            }
            CachedProviderClient::Anthropic(c) => visitor.visit(c.completion_model(model_name)),
            CachedProviderClient::Ollama(c) => visitor.visit(c.completion_model(model_name)),
            #[cfg(test)]
            CachedProviderClient::Mock(m) => visitor.visit(m.clone()),
        }
    }
}

pub type ClientCacheKey = (String, Option<String>, Option<String>);

#[cfg(test)]
#[path = "providers_test.rs"]
mod providers_test;
