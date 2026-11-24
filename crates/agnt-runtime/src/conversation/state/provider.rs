use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::super::managers::ProviderManager;
use super::super::providers::{
    CachedProviderClient, ClientCacheKey, build_anthropic_client, build_copilot_client,
    build_ollama_client, build_openai_client,
};
use crate::config::models_cache::ModelCost;
use crate::config::pricing::PricingView;
use crate::config::{Config, ModelRoleConfig, PluginConfig};
use nu_protocol::LabeledError;

pub struct ProviderState {
    config: Config,
    startup_plugin_config: Option<PluginConfig>,
    /// The resolved Copilot credential directory; `None` falls back to the
    /// platform default.
    copilot_token_dir: Option<std::path::PathBuf>,
    cached_client: Option<CachedProviderClient>,
    cached_client_key: Option<ClientCacheKey>,
    /// The vendor the cached client's wire reports: the name rig files the
    /// model's facts and spec under. `None` before a build, when the client was
    /// invalidated, and for a wire that names no vendor (the mock).
    wire_vendor: Option<String>,
}

impl ProviderState {
    pub fn new(config: Config, startup_plugin_config: Option<PluginConfig>) -> Self {
        Self {
            config,
            startup_plugin_config,
            copilot_token_dir: None,
            cached_client: None,
            cached_client_key: None,
            wire_vendor: None,
        }
    }

    /// Inject the resolved Copilot credential directory
    /// ([`Paths::copilot_token_dir`](crate::config::Paths::copilot_token_dir)).
    pub fn with_copilot_token_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.copilot_token_dir = Some(dir.into());
        self
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
        let provider_type = self.config.provider_kind.as_deref().unwrap_or(provider_key);
        log::info!(
            "creating {} client (type={}) for model={}",
            provider_key,
            provider_type,
            self.config.model
        );
        let client = match provider_type {
            "copilot" | "github-copilot" | "github_copilot" => CachedProviderClient::Copilot(
                build_copilot_client(&self.config, self.copilot_token_dir.as_deref()).await?,
            ),
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
        // The new client has not built a wire yet: the vendor its wire reports
        // is unknown until the next build.
        self.set_wire_vendor(None);
        Ok(())
    }

    pub fn invalidate_cache(&mut self) {
        self.cached_client = None;
        self.cached_client_key = None;
        self.set_wire_vendor(None);
    }

    /// The vendor the built wire reports, `None` before a build.
    ///
    /// This is the name rig files the model's facts and spec under, so a
    /// consumer that resolves a spec takes it before the config's own spelling.
    /// Crate-only: the runtime is the one reader, and
    /// [`ProviderManager::wire_vendor`](crate::conversation::managers::ProviderManager::wire_vendor)
    /// is the seam it reads through.
    pub(crate) fn wire_vendor(&self) -> Option<&str> {
        self.wire_vendor.as_deref()
    }

    /// Record the vendor the wire a build landed on reports.
    fn set_wire_vendor(&mut self, vendor: Option<String>) {
        self.wire_vendor = vendor;
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
    ///
    /// Answers the model and the vendor its wire reports, and keeps that vendor
    /// (`Self::wire_vendor`) for the spec resolutions that follow the build.
    pub async fn build_dyn_model(
        &mut self,
    ) -> Result<(rig::DynModel<rig::operation::Completion>, Option<String>), LabeledError> {
        self.ensure_client_cached().await?;
        let client = self
            .cached_client
            .as_ref()
            .ok_or_else(|| LabeledError::new("client must be cached after ensure_client_cached"))?;
        let (pricing, config_pricing) = self.model_pricing();
        let (model, wire_vendor) = client.build_dyn_model(
            &self.config.model,
            self.config.cache_section(),
            pricing.as_ref(),
            config_pricing,
        )?;
        self.set_wire_vendor(wire_vendor.clone());
        Ok((model, wire_vendor))
    }

    /// The price ladder over the startup models cache and the configured price
    /// for the active model: what a wire is bound to when it is built.
    fn model_pricing(&self) -> (Option<PricingView<'_>>, Option<&ModelCost>) {
        let cache = self
            .startup_plugin_config
            .as_ref()
            .and_then(|config| config.models_cache.as_ref());
        (cache.map(PricingView::new), self.config.pricing.as_ref())
    }
}

// region:    --- ModelSlot

/// The erased completion model, and whether it still matches the active config.
///
/// The runtime and the compactor share one slot: every turn's agent is built
/// from it, and the compactor summarizes with it. A model switch only marks the
/// slot stale ([`Self::mark_pending`]); the provider client and the model erased
/// from it build at the slot's next use ([`Self::ensure`]), never inside the
/// switch. A switch therefore awaits no transport: a provider whose client takes
/// seconds to answer, or never answers, cannot wedge the surface.
///
/// Three states, one owner, two consumers (the turn executor and the compactor):
///
/// - *built*: the slot holds the model the active config names.
/// - *pending*: a switch left it stale, so its next use builds.
/// - *building*: a consumer owns the build, and the slot is being written.
///
/// The build is the only slow step and only one consumer may own it, so a
/// second consumer that finds the slot pending or building YIELDS
/// ([`Self::try_built`] answers `None`) instead of waiting: a slow or wedged
/// build then holds nobody but the consumer that started it.
#[derive(Clone)]
pub struct ModelSlot {
    cell: Arc<Mutex<rig::DynModel<rig::operation::Completion>>>,
    pending: Arc<AtomicBool>,
    building: Arc<AtomicBool>,
}

impl ModelSlot {
    /// A slot holding a built model, with nothing pending and nothing building.
    pub fn from_model(model: rig::DynModel<rig::operation::Completion>) -> Self {
        Self {
            cell: Arc::new(Mutex::new(model)),
            pending: Arc::new(AtomicBool::new(false)),
            building: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The model the slot holds now, cloned.
    ///
    /// After [`Self::mark_pending`] this is the model in use before the switch:
    /// an in-flight turn keeps it, and a reader that asks before the next use
    /// sees the previous identity. Neither can be served from an empty slot,
    /// which is why the cell keeps the superseded model rather than clearing.
    pub fn model(&self) -> rig::DynModel<rig::operation::Completion> {
        self.lock().clone()
    }

    /// The shared model cell, for a consumer that holds the raw handle (the
    /// turn executor and the compactor).
    pub fn cell(&self) -> Arc<Mutex<rig::DynModel<rig::operation::Completion>>> {
        Arc::clone(&self.cell)
    }

    /// Whether a switch left the slot stale, so its next use must build.
    pub fn is_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }

    /// Whether a consumer is building the slot right now.
    fn is_building(&self) -> bool {
        self.building.load(Ordering::SeqCst)
    }

    /// Record that the config no longer matches the model in the slot.
    ///
    /// This is a switch's only model write: no client is created and nothing is
    /// awaited.
    pub fn mark_pending(&self) {
        self.pending.store(true, Ordering::SeqCst);
    }

    /// Record that this consumer owns the build.
    ///
    /// A second consumer reads the slot through [`Self::try_built`] and yields
    /// on it, so the build has one owner and no reader waits for it.
    pub(crate) fn mark_building(&self) {
        self.building.store(true, Ordering::SeqCst);
    }

    /// The model the slot holds when it is built and fresh.
    ///
    /// `None` while a switch left the slot stale or a consumer is building it:
    /// a reader that cannot be served yields rather than waits, so a slow or
    /// wedged build holds only the consumer that started it.
    pub(crate) fn try_built(&self) -> Option<rig::DynModel<rig::operation::Completion>> {
        (!self.is_pending() && !self.is_building()).then(|| self.model())
    }

    /// Install `model` and clear the staleness the build answered.
    ///
    /// The install is the build's own write, so it releases the ownership too.
    pub fn install(&self, model: rig::DynModel<rig::operation::Completion>) {
        *self.lock() = model;
        self.pending.store(false, Ordering::SeqCst);
        self.building.store(false, Ordering::SeqCst);
    }

    /// The model the active config names, built when the slot is stale.
    ///
    /// `build` is the provider's own construction ([`ProviderState::build_dyn_model`]),
    /// so the provider client is created here, at first use. This is the
    /// awaiting form: the build belongs to the caller that asked for it. A
    /// failed build releases the ownership and leaves the slot stale, so the
    /// next use tries again.
    pub async fn ensure<F, Fut>(
        &self,
        build: F,
    ) -> Result<rig::DynModel<rig::operation::Completion>, LabeledError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<
                Output = Result<rig::DynModel<rig::operation::Completion>, LabeledError>,
            >,
    {
        if !self.is_pending() {
            return Ok(self.model());
        }
        self.mark_building();
        let model = match build().await {
            Ok(model) => model,
            Err(e) => {
                self.release_building();
                return Err(e);
            }
        };
        self.install(model.clone());
        Ok(model)
    }

    /// Give the build ownership back after a build that landed no model.
    fn release_building(&self) {
        self.building.store(false, Ordering::SeqCst);
    }

    /// The shared cell, recovered if a switch panicked mid-write.
    fn lock(&self) -> std::sync::MutexGuard<'_, rig::DynModel<rig::operation::Completion>> {
        self.cell.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// endregion: --- ModelSlot

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
        ProviderState::build_dyn_model(self)
            .await
            .map(|(model, _wire_vendor)| model)
    }

    fn wire_vendor(&self) -> Option<&str> {
        ProviderState::wire_vendor(self)
    }

    fn startup_plugin_config(&self) -> Option<&PluginConfig> {
        self.startup_plugin_config.as_ref()
    }
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/state/provider.rs"]
mod provider_test;

// endregion: --- Tests
