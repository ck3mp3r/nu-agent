use crate::config::models_cache::ModelCost;
use crate::config::pricing::{PricingView, ResolvedPricing};
use crate::config::{ModelsCache, PluginConfig};
use crate::protocol::persona::PersonaSummary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPickerOption {
    pub provider: String,
    pub model: String,
    pub identity: String,
    pub display: String,
    pub active: bool,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub cost: Option<String>,
    pub configured: bool,
    pub provider_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPickerOption {
    pub name: String,
    pub description: Option<String>,
    pub display: String,
    pub builtin: bool,
}

// endregion: --- Types

// region:    --- Functions

/// Format the active model identity as `provider/model`.
///
/// Returns the model unchanged when it already carries the provider prefix.
pub fn format_active_model_identity(provider: &str, model: &str) -> String {
    if model.starts_with(&format!("{provider}/")) {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

/// Build the model picker catalog from the models cache and the configured providers.
pub fn build_model_picker_catalog(
    models_cache: Option<&ModelsCache>,
    plugin_config: &PluginConfig,
    active_model_identity: &str,
) -> Vec<ModelPickerOption> {
    let mut options: Vec<ModelPickerOption> = Vec::new();
    // The cost column reads the pricing ladder, so a model the session prices
    // by a configured price or by its own catalog row shows that price here
    // too.
    let pricing = models_cache.map(PricingView::new);
    let cost_of = |provider_id: &str, model_id: &str, config_pricing: Option<&ModelCost>| {
        match pricing.as_ref() {
            Some(view) => view.pricing_for(provider_id, model_id, config_pricing),
            None => ResolvedPricing::from_config(config_pricing),
        }
        .pricing
        .map(|pricing| format!("${:.2}/${:.2}", pricing.input, pricing.output))
    };

    // Only show models from providers that have a provider block in config.toml.
    // A block whose resolved cache section exists is a full-catalog endpoint,
    // so every section row is runnable through it. A block with no section
    // serves only what it declares. The cache section supplies metadata either
    // way, joined by model id.
    for (provider_id, provider_cfg) in &plugin_config.providers {
        let section = plugin_config.cache_section(provider_id);
        let provider_spec = models_cache.and_then(|cache| cache.provider_spec(section));
        let section_models = provider_spec.map(|spec| &spec.models);

        let mut model_ids: Vec<&str> = section_models
            .map(|models| models.keys().map(String::as_str).collect())
            .unwrap_or_default();
        for model_id in provider_cfg.models.keys() {
            if !model_ids.contains(&model_id.as_str()) {
                model_ids.push(model_id.as_str());
            }
        }

        for model_id in model_ids {
            let identity = format!("{provider_id}/{model_id}");
            let model_spec = provider_spec.and_then(|spec| spec.models.get(model_id));
            let model_name = model_spec
                .map(|spec| spec.name.as_str())
                .unwrap_or(model_id);
            let model_cfg = provider_cfg.models.get(model_id);
            options.push(ModelPickerOption {
                provider: provider_id.clone(),
                model: model_id.to_string(),
                identity: identity.clone(),
                display: format!("{provider_id} / {model_name}"),
                active: identity == active_model_identity,
                context_window: model_spec.map(|spec| spec.limit.context),
                max_output: model_spec.map(|spec| spec.limit.output),
                cost: cost_of(
                    section,
                    model_id,
                    model_cfg.and_then(|cfg| cfg.pricing.as_ref()),
                ),
                configured: model_cfg.is_some(),
                provider_display_name: provider_id.clone(),
            });
        }
    }

    // Sort: by provider, then configured first, then alphabetical
    options.sort_by(|a, b| {
        a.provider
            .to_ascii_lowercase()
            .cmp(&b.provider.to_ascii_lowercase())
            .then_with(|| b.configured.cmp(&a.configured))
            .then_with(|| {
                a.model
                    .to_ascii_lowercase()
                    .cmp(&b.model.to_ascii_lowercase())
            })
    });

    options
}

/// Build the model picker catalog from the runtime's cached startup plugin config.
pub fn model_picker_catalog_from_cached_startup_plugin_config(
    startup_plugin_config: Option<&PluginConfig>,
    active_model_identity: &str,
) -> Vec<ModelPickerOption> {
    let Some(config) = startup_plugin_config else {
        return Vec::new();
    };
    build_model_picker_catalog(config.models_cache.as_ref(), config, active_model_identity)
}

/// Build the agent picker catalog from the available persona summaries.
pub fn build_agent_picker_catalog(available_agents: &[PersonaSummary]) -> Vec<AgentPickerOption> {
    available_agents
        .iter()
        .map(|agent| {
            let display = if agent.builtin {
                match &agent.description {
                    Some(desc) => format!("{} — {} [built-in]", agent.name, desc),
                    None => format!("{} [built-in]", agent.name),
                }
            } else {
                match &agent.description {
                    Some(desc) => format!("{} — {}", agent.name, desc),
                    None => agent.name.clone(),
                }
            };
            AgentPickerOption {
                name: agent.name.clone(),
                description: agent.description.clone(),
                display,
                builtin: agent.builtin,
            }
        })
        .collect()
}

// endregion: --- Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/protocol/picker.rs"]
mod picker_test;

// endregion: --- Tests
