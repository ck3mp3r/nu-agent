use nu_agent_core::config::{ModelsCache, PluginConfig};
use nu_agent_core::protocol::persona::PersonaSummary;
use nu_agent_core::protocol::picker::{AgentPickerOption, ModelPickerOption};

pub(crate) fn format_active_model_identity(provider: &str, model: &str) -> String {
    if model.starts_with(&format!("{provider}/")) {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

pub(crate) fn build_model_picker_catalog(
    models_cache: Option<&ModelsCache>,
    plugin_config: &PluginConfig,
    active_model_identity: &str,
) -> Vec<ModelPickerOption> {
    let mut options: Vec<ModelPickerOption> = Vec::new();

    // Only show models from providers that have a provider block in config.toml
    for (provider_id, provider_cfg) in &plugin_config.providers {
        let provider_display_name = if let Some(cache) = models_cache
            && let Some(spec) = cache.providers.get(provider_id)
        {
            spec.name.clone()
        } else {
            provider_id.clone()
        };

        // If cache is available, show ALL models for this provider (not just configured ones)
        if let Some(cache) = models_cache
            && let Some(provider_spec) = cache.providers.get(provider_id)
        {
            for (model_id, model_spec) in &provider_spec.models {
                let identity = format!("{provider_id}/{model_id}");
                let configured = provider_cfg.models.contains_key(model_id);
                options.push(ModelPickerOption {
                    provider: provider_id.clone(),
                    model: model_id.clone(),
                    identity: identity.clone(),
                    display: format!("{} / {}", provider_spec.name, model_spec.name),
                    active: identity == active_model_identity,
                    context_window: Some(model_spec.limit.context),
                    max_output: Some(model_spec.limit.output),
                    configured,
                    provider_display_name: provider_spec.name.clone(),
                });
            }
        } else {
            // No cache — show only configured models for this provider
            for model_id in provider_cfg.models.keys() {
                let identity = format!("{provider_id}/{model_id}");
                options.push(ModelPickerOption {
                    provider: provider_id.clone(),
                    model: model_id.clone(),
                    identity: identity.clone(),
                    display: format!("{provider_id} / {model_id}"),
                    active: identity == active_model_identity,
                    context_window: None,
                    max_output: None,
                    configured: true,
                    provider_display_name: provider_display_name.clone(),
                });
            }
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

pub(crate) fn model_picker_catalog_from_cached_startup_plugin_config(
    startup_plugin_config: Option<&PluginConfig>,
    active_model_identity: &str,
) -> Vec<ModelPickerOption> {
    let Some(config) = startup_plugin_config else {
        return Vec::new();
    };
    build_model_picker_catalog(config.models_cache.as_ref(), config, active_model_identity)
}

pub(crate) fn build_agent_picker_catalog(
    available_agents: &[PersonaSummary],
    active_agent: Option<&str>,
) -> Vec<AgentPickerOption> {
    available_agents
        .iter()
        .map(|agent| {
            let active = active_agent.is_some_and(|a| a == agent.name);
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
                active,
                builtin: agent.builtin,
            }
        })
        .collect()
}
