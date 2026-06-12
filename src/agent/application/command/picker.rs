use crate::agent::protocol::persona::PersonaSummary;
use crate::agent::ui::tui::state::{AgentPickerOption, ModelPickerOption};
use crate::config::PluginConfig;

pub(crate) fn format_active_model_identity(provider: &str, model: &str) -> String {
    if model.starts_with(&format!("{provider}/")) {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

pub(crate) fn build_model_picker_catalog_from_plugin_config(
    plugin_config: &PluginConfig,
    active_model_identity: &str,
) -> Vec<ModelPickerOption> {
    let mut options = plugin_config
        .providers
        .iter()
        .flat_map(|(provider, provider_config)| {
            provider_config.models.keys().map(move |model| {
                let identity = format!("{provider}/{model}");
                ModelPickerOption {
                    provider: provider.clone(),
                    model: model.clone(),
                    identity: identity.clone(),
                    display: format!("{provider} / {model}"),
                    active: identity == active_model_identity,
                }
            })
        })
        .collect::<Vec<_>>();

    options.sort_by(|left, right| {
        left.provider
            .to_ascii_lowercase()
            .cmp(&right.provider.to_ascii_lowercase())
            .then_with(|| {
                left.model
                    .to_ascii_lowercase()
                    .cmp(&right.model.to_ascii_lowercase())
            })
    });
    options
}

pub(crate) fn model_picker_catalog_from_cached_startup_plugin_config(
    startup_plugin_config: Option<&PluginConfig>,
    active_model_identity: &str,
) -> Vec<ModelPickerOption> {
    startup_plugin_config
        .map(|config| build_model_picker_catalog_from_plugin_config(config, active_model_identity))
        .unwrap_or_default()
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
