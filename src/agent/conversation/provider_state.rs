use super::providers::{
    CachedProviderClient, ClientCacheKey, build_anthropic_client, build_copilot_client,
    build_ollama_client, build_openai_client, resolve_provider_type,
};
use crate::config::{Config, PluginConfig};
use nu_protocol::LabeledError;

pub(crate) struct ProviderState {
    config: Config,
    startup_plugin_config: Option<PluginConfig>,
    cached_client: Option<CachedProviderClient>,
    cached_client_key: Option<ClientCacheKey>,
}

impl ProviderState {
    pub(crate) fn new(config: Config, startup_plugin_config: Option<PluginConfig>) -> Self {
        Self {
            config,
            startup_plugin_config,
            cached_client: None,
            cached_client_key: None,
        }
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn cache_key(&self) -> ClientCacheKey {
        (
            self.config.provider.clone(),
            self.config.api_key.clone(),
            self.config.base_url.clone(),
        )
    }

    /// Take the cached client out temporarily (caller must restore via restore_client).
    pub(crate) fn take_client(&mut self) -> Option<CachedProviderClient> {
        self.cached_client.take()
    }

    /// Restore a client taken via take_client.
    pub(crate) fn restore_client(&mut self, client: CachedProviderClient) {
        self.cached_client = Some(client);
    }

    /// Return a reference to the cached client (if any).
    pub(crate) fn client(&self) -> Option<&CachedProviderClient> {
        self.cached_client.as_ref()
    }

    pub(crate) fn ensure_client_cached(&mut self) -> Result<(), LabeledError> {
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
            "copilot" | "github-copilot" | "github_copilot" => CachedProviderClient::Copilot(build_copilot_client(&self.config)?),
            "openai" => CachedProviderClient::OpenAi(build_openai_client(&self.config)?),
            "anthropic" => CachedProviderClient::Anthropic(build_anthropic_client(&self.config)?),
            "ollama" => CachedProviderClient::Ollama(build_ollama_client(&self.config)?),
            other => return Err(LabeledError::new(format!("Unsupported provider: '{}' (from config key '{}')", other, provider_key))
                .with_help("Supported: copilot, openai, anthropic, ollama. Set 'provider' field in provider config to map custom names.")),
        };
        self.cached_client = Some(client);
        self.cached_client_key = Some(key);
        Ok(())
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.cached_client = None;
        self.cached_client_key = None;
    }

    pub(crate) fn startup_plugin_config(&self) -> Option<&PluginConfig> {
        self.startup_plugin_config.as_ref()
    }

    pub(crate) fn switch_model(&mut self, model_spec: &str) -> Result<String, String> {
        let plugin_config = self.startup_plugin_config.clone().ok_or_else(|| {
            "model switch unavailable: startup plugin config cache is missing".to_string()
        })?;
        let resolved = plugin_config.resolve_model(model_spec)?;
        self.config = resolved;
        self.invalidate_cache();
        Ok(format!("{}/{}", self.config.provider, self.config.model))
    }
}
