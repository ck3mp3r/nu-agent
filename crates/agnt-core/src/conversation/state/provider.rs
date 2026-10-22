use super::super::managers::ProviderManager;
use super::super::providers::{
    CachedProviderClient, ClientCacheKey, build_anthropic_client, build_copilot_client,
    build_ollama_client, build_openai_client, resolve_provider_type,
};
use crate::config::pricing::PricingView;
use crate::config::{Config, ModelRoleConfig, PluginConfig};
use nu_protocol::LabeledError;

pub struct ProviderState {
    config: Config,
    startup_plugin_config: Option<PluginConfig>,
    cached_client: Option<CachedProviderClient>,
    cached_client_key: Option<ClientCacheKey>,
}

impl ProviderState {
    pub fn new(config: Config, startup_plugin_config: Option<PluginConfig>) -> Self {
        Self {
            config,
            startup_plugin_config,
            cached_client: None,
            cached_client_key: None,
        }
    }

    /// Construct a provider state with a pre-built client.
    ///
    /// The client is cached under the config's cache key, so
    /// [`Self::ensure_client_cached`] is a no-op and [`Self::build_dyn_model`]
    /// builds from the injected client instead of a transport client. Use this
    /// to drive the runtime with a substitute client (for example a mock model)
    /// without building a real HTTP transport.
    pub fn with_client(config: Config, client: CachedProviderClient) -> Self {
        let mut state = Self::new(config, None);
        state.cached_client_key = Some(state.cache_key());
        state.cached_client = Some(client);
        state
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn cache_key(&self) -> ClientCacheKey {
        (
            self.config.provider.clone(),
            self.config.api_key.clone(),
            self.config.base_url.clone(),
            self.config.read_timeout_secs,
        )
    }

    /// Return a reference to the cached client (if any).
    pub fn client(&self) -> Option<&CachedProviderClient> {
        self.cached_client.as_ref()
    }

    pub async fn ensure_client_cached(&mut self) -> Result<(), LabeledError> {
        let key = self.cache_key();
        if self.cached_client_key.as_ref() == Some(&key) {
            return Ok(());
        }
        let provider_key = self.config.provider.as_str();
        let provider_type =
            resolve_provider_type(provider_key, self.config.provider_impl.as_deref());
        log::info!(
            "creating {} client (type={}) for model={}",
            provider_key,
            provider_type,
            self.config.model
        );
        let client = match provider_type {
            "copilot" | "github-copilot" | "github_copilot" => CachedProviderClient::Copilot(build_copilot_client(&self.config).await?),
            "openai" => {
                let client = build_openai_client(&self.config)?;
                // rig 0.43 carries the route in the client's `OpenAIConfig`:
                // `build_openai_client` already selects `Route::Chat` when a
                // `base_url` is set, so both variants hold the same client type.
                if self.config.base_url.is_some() {
                    CachedProviderClient::OpenAiCompletions(client)
                } else {
                    CachedProviderClient::OpenAi(client)
                }
            }
            "anthropic" => CachedProviderClient::Anthropic(build_anthropic_client(&self.config)?),
            "ollama" => CachedProviderClient::Ollama(build_ollama_client(&self.config)?),
            other => return Err(LabeledError::new(format!("Unsupported provider: '{other}' (from config key '{provider_key}')"))
                .with_help("Supported: copilot, openai, anthropic, ollama. Set 'provider' field in provider config to map custom names.")),
        };
        self.cached_client = Some(client);
        self.cached_client_key = Some(key);
        Ok(())
    }

    pub fn invalidate_cache(&mut self) {
        self.cached_client = None;
        self.cached_client_key = None;
    }

    pub fn startup_plugin_config(&self) -> Option<&PluginConfig> {
        self.startup_plugin_config.as_ref()
    }

    pub fn switch_model(&mut self, model_spec: &str) -> Result<String, String> {
        let plugin_config = self.startup_plugin_config.clone().ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })?;
        let role_config = ModelRoleConfig {
            model: model_spec.to_string(),
            ..Default::default()
        };
        let resolved = plugin_config.resolve_model(&role_config)?;
        self.config = resolved;
        self.invalidate_cache();
        Ok(format!("{}/{}", self.config.provider, self.config.model))
    }

    /// Canonical constructor for the erased completion model.
    ///
    /// Ensures the client is cached for the current config, then erases the
    /// completion model into a `DynModel<Completion>` built from the **bare**
    /// model name (`self.config.model`). Both startup and model switching route
    /// through it so the bare-name extraction lives in exactly one place.
    ///
    /// The model is bound to the pricing ladder over the startup models cache,
    /// so `DynModel::spec()` answers with the ladder's prices. Without a cache
    /// the configured price still applies; with neither, the wire keeps the
    /// built-in catalog's answer.
    pub async fn build_dyn_model(
        &mut self,
    ) -> Result<rig::DynModel<rig::operation::Completion>, LabeledError> {
        self.ensure_client_cached().await?;
        let client = self
            .cached_client
            .as_ref()
            .ok_or_else(|| LabeledError::new("client must be cached after ensure_client_cached"))?;
        let cache = self
            .startup_plugin_config
            .as_ref()
            .and_then(|config| config.models_cache.as_ref());
        let pricing = cache.map(PricingView::new);
        client.build_dyn_model(
            &self.config.model,
            self.config.cache_section(),
            pricing.as_ref(),
            self.config.pricing.as_ref(),
        )
    }
}

impl ProviderManager for ProviderState {
    fn provider_config(&self) -> &Config {
        &self.config
    }

    fn provider_config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    async fn ensure_client_cached(&mut self) -> Result<(), LabeledError> {
        self.ensure_client_cached().await
    }

    fn cached_client(&self) -> Option<&CachedProviderClient> {
        self.cached_client.as_ref()
    }

    fn invalidate_cache(&mut self) {
        self.invalidate_cache()
    }

    fn switch_model(&mut self, model_spec: &str) -> Result<String, String> {
        self.switch_model(model_spec)
    }

    async fn build_dyn_model(
        &mut self,
    ) -> Result<rig::DynModel<rig::operation::Completion>, LabeledError> {
        self.build_dyn_model().await
    }

    fn startup_plugin_config(&self) -> Option<&PluginConfig> {
        self.startup_plugin_config.as_ref()
    }
}
