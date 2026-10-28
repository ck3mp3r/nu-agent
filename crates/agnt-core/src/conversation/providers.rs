use std::path::{Path, PathBuf};
use std::time::Duration;

use nu_protocol::LabeledError;
use rig::catalog::{Catalog, ModelFacts, ModelSpec as RigModelSpec};
use rig::http_client::{HttpClientExt, ReqwestClient};
use rig::providers::copilot;
use rig::providers::copilot::auth::{
    AuthError, AuthSource, Authenticator, DeviceCodeHandler, FileTokenStore,
};
use rig::providers::registry::ProviderId;

use crate::config::copilot_tokens::copilot_token_dir;
use crate::config::models_cache::{
    Modalities as CacheModalities, ModelCost, ModelSpec as CacheModelSpec,
};
use crate::config::pricing::{PricingView, ResolvedPricing};
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
///
/// Copilot is an OpenAI-shaped provider in rig 0.45: the client is an `OpenAI`
/// built on the Copilot dialect.
///
/// `token_dir` is the resolved credential directory
/// ([`Paths::copilot_token_dir`](crate::config::Paths::copilot_token_dir));
/// `None` leaves the legacy platform default in use.
pub(super) async fn build_copilot_client(
    config: &Config,
    token_dir: Option<&Path>,
) -> Result<rig::providers::openai::OpenAI, LabeledError> {
    build_copilot_client_with(config, &process_env(), token_dir).await
}

/// Build a GitHub Copilot client against an explicit environment map.
///
/// `token_dir` overrides the credential directory. When it is absent, the
/// platform default (`$XDG_CONFIG_HOME/{COPILOT_DIR_NAME}`, which is
/// `github_copilot-dev` in debug builds and `github_copilot` in release builds)
/// is used.
pub(super) async fn build_copilot_client_with(
    config: &Config,
    env: &EnvMap,
    token_dir: Option<&Path>,
) -> Result<rig::providers::openai::OpenAI, LabeledError> {
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
/// the platform default. `None` means no directory can hold credentials.
fn resolve_token_dir(token_dir: Option<&Path>) -> Option<PathBuf> {
    token_dir.map(Path::to_path_buf).or_else(copilot_token_dir)
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
) -> Result<rig::providers::openai::OpenAI, LabeledError>
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

    let Some(source) = source else {
        log::debug!("Copilot auth: using a configured Copilot API key");
        return Ok(copilot_client(
            static_key.unwrap_or_default(),
            base_url,
            http,
        ));
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
        FileTokenStore::new(&token_dir),
        DeviceCodeHandler::default(),
        false,
    );

    let context = authenticator
        .auth_context(&http)
        .await
        .map_err(copilot_auth_error)?;

    Ok(copilot_client(
        context.api_key.expose().to_string(),
        base_url.or(context.api_base),
        http,
    ))
}

/// Connect an `OpenAI` client on the Copilot dialect.
fn copilot_client<H>(
    api_key: String,
    base_url: Option<String>,
    http: H,
) -> rig::providers::openai::OpenAI
where
    H: HttpClientExt + 'static,
{
    let mut config = rig::providers::openai::OpenAIConfig::with_key(&copilot::DIALECT, api_key);
    if let Some(url) = base_url {
        config = config.with_base_url(url);
    }
    config.connect(http)
}

/// Run the GitHub Copilot OAuth device-code flow.
///
/// Persists the credential record under `token_dir`
/// ([`Paths::copilot_token_dir`](crate::config::Paths::copilot_token_dir)) and
/// returns it. The device flow is allowed here — this is the interactive
/// sign-in path, unlike the runtime client builder.
pub async fn copilot_device_login(
    handler: DeviceCodeHandler,
    token_dir: PathBuf,
) -> Result<PathBuf, LabeledError> {
    let http_client = build_http_client(None)?;
    let authenticator = Authenticator::new(
        AuthSource::OAuth,
        FileTokenStore::new(&token_dir),
        handler,
        true,
    );

    authenticator
        .auth_context(&ReqwestClient::from(http_client))
        .await
        .map_err(copilot_auth_error)?;

    Ok(token_dir)
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
    /// GitHub Copilot is an OpenAI-shaped provider in rig 0.45, so it holds the
    /// same client type as [`Self::OpenAi`], connected on the Copilot dialect.
    Copilot(rig::providers::openai::OpenAI),
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
    ///
    /// `pricing` is the ladder over the models cache, `cache_section` the
    /// cache section the model's block resolves to (its declared `catalog`,
    /// else its name), and `config_pricing` the user's price for the model.
    /// When either price source is given, the wire is bound to the model's
    /// resolved spec, so `DynModel::spec()` answers with the ladder's prices
    /// instead of the built-in catalog's. With neither, the wire keeps the
    /// built-in catalog's answer.
    pub fn build_dyn_model(
        &self,
        model_name: &str,
        cache_section: &str,
        pricing: Option<&PricingView<'_>>,
        config_pricing: Option<&ModelCost>,
    ) -> Result<rig::DynModel<rig::operation::Completion>, nu_protocol::LabeledError> {
        log::debug!("build_dyn_model: model={model_name}");
        let model = match self {
            CachedProviderClient::Copilot(c) => bind_and_erase(
                c.completion(model_name),
                model_name,
                cache_section,
                pricing,
                config_pricing,
            ),
            CachedProviderClient::OpenAi(c) => bind_and_erase(
                c.completion(model_name),
                model_name,
                cache_section,
                pricing,
                config_pricing,
            ),
            CachedProviderClient::OpenAiCompletions(c) => bind_and_erase(
                c.completion(model_name),
                model_name,
                cache_section,
                pricing,
                config_pricing,
            ),
            CachedProviderClient::Anthropic(c) => bind_and_erase(
                c.completion(model_name),
                model_name,
                cache_section,
                pricing,
                config_pricing,
            ),
            CachedProviderClient::Ollama(c) => bind_and_erase(
                c.completion(model_name),
                model_name,
                cache_section,
                pricing,
                config_pricing,
            ),
            // The mock wire names no vendor, and tests price it with
            // `priced_as`, so it takes no bound facts.
            CachedProviderClient::Mock(m) => m.clone().erase(),
        };
        Ok(model)
    }
}

/// A completion wire that takes the facts it encodes and prices with.
///
/// rig declares `with_facts` per wire type rather than on a trait, so this
/// names the three wires the cached clients build. It exists only to let
/// [`bind_and_erase`] stay generic over the wire.
trait BindFacts {
    fn bind_facts(self, facts: ModelFacts) -> Self;
}

impl BindFacts for rig::providers::openai::wire::OpenAiWire {
    fn bind_facts(self, facts: ModelFacts) -> Self {
        self.with_facts(facts)
    }
}

impl BindFacts for rig::providers::anthropic::wire::Messages {
    fn bind_facts(self, facts: ModelFacts) -> Self {
        self.with_facts(facts)
    }
}

impl BindFacts for rig::providers::openai::wire::Chat {
    fn bind_facts(self, facts: ModelFacts) -> Self {
        self.with_facts(facts)
    }
}

/// Bind the model's resolved facts to its wire, then erase it.
///
/// The vendor is the wire's own `describe().name`, so the wire owns the name
/// its facts are filed under. A parallel per-provider table could drift from
/// the wire and make `ModelFacts::for_model`'s bound-path check miss silently.
fn bind_and_erase<W, T>(
    model: rig::Model<W, T>,
    model_name: &str,
    cache_section: &str,
    pricing: Option<&PricingView<'_>>,
    config_pricing: Option<&ModelCost>,
) -> rig::DynModel<rig::operation::Completion>
where
    W: rig::wire::Wire<Op = rig::operation::Completion> + BindFacts,
    T: rig::driver::Transport<W>,
{
    // Owned so the wire borrow ends before the facts are bound to it.
    let vendor = model.wire.describe().name.to_string();
    let mut model = model;
    if let Some(facts) = model_facts(&vendor, model_name, cache_section, pricing, config_pricing) {
        model.wire = model.wire.bind_facts(facts);
    }
    model.erase()
}

/// The facts the wire encodes and prices with: the model's catalog spec
/// with the ladder's prices laid on it.
///
/// The spec is filed under `vendor`, the name the wire reports, because rig
/// answers a wire's `spec()` only for a spec filed under that vendor.
///
/// `None` when there is no ladder to consult and no configured price, so the
/// wire keeps the built-in catalog's answer rather than losing the prices rig
/// already knows.
fn model_facts(
    vendor: &str,
    model_name: &str,
    cache_section: &str,
    pricing: Option<&PricingView<'_>>,
    config_pricing: Option<&ModelCost>,
) -> Option<ModelFacts> {
    if pricing.is_none() && config_pricing.is_none() {
        return None;
    }
    let id = ProviderId::catalog(vendor)?;
    let cache_spec = pricing.and_then(|view| view.cache_spec(cache_section, model_name));
    let mut spec = base_spec(id, model_name, cache_spec);
    spec.pricing = match pricing {
        Some(view) => view.pricing_for(cache_section, model_name, config_pricing),
        None => ResolvedPricing::from_config(config_pricing),
    }
    .pricing;
    Some(ModelFacts::new(spec))
}

/// The spec the wire is bound to: rig's built-in row when it lists the model,
/// else a minimal spec built from the cache row.
fn base_spec(
    id: ProviderId,
    model_name: &str,
    cache_spec: Option<&CacheModelSpec>,
) -> RigModelSpec {
    if let Some(resolved) = Catalog::builtin().get(id, model_name) {
        return resolved.spec.clone();
    }
    let mut spec = RigModelSpec::new(id, model_name);
    let Some(cache_spec) = cache_spec else {
        return spec;
    };
    spec = spec
        .with_display_name(&cache_spec.name)
        .with_context_window(cache_spec.limit.context)
        .with_max_output_tokens(cache_spec.limit.output)
        .with_tools(cache_spec.tool_call);
    if let Some(modalities) = &cache_spec.modalities {
        apply_modalities(&mut spec, modalities);
    }
    spec
}

/// Copy the cache row's input modalities onto the spec, so a wire that filters
/// images reads the same answer the cache lists.
fn apply_modalities(spec: &mut RigModelSpec, modalities: &CacheModalities) {
    let reads = |name: &str| modalities.input.iter().any(|listed| listed == name);
    spec.input.text = reads("text");
    spec.input.image = reads("image");
    spec.input.audio = reads("audio");
    spec.input.video = reads("video");
    spec.input.pdf = reads("pdf");
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
