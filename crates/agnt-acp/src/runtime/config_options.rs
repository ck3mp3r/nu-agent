//! The model and agent selectors advertised to ACP clients.

use std::path::Path;

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
};
use agnt_runtime::config::{AgentsConfig, Config, ModelsCache};
use agnt_runtime::protocol::persona::{FsPersonaResolver, PersonaLister};

/// The id of the model selector config option.
pub const MODEL_CONFIG_ID: &str = "model";

/// The id of the agent selector config option.
pub const AGENT_CONFIG_ID: &str = "agent";

/// Build the model selector advertised to ACP clients.
///
/// With a models cache, every model of the configured provider is selectable.
/// Without one — or when the cache has no entry for the provider — only the
/// currently configured model is advertised, so the client still renders a
/// selector for the active model.
pub fn build_model_config_options(
    config: &Config,
    cache: Option<&ModelsCache>,
) -> Vec<SessionConfigOption> {
    let current = model_value(config);
    let mut options: Vec<SessionConfigSelectOption> = cache
        .map(|cache| {
            cache
                .list_models(Some(config.cache_section()))
                .into_iter()
                // The value is the config's identity, not the cache section
                // key: a block named `ollama` serving the `ollama-cloud`
                // section advertises `ollama/<model>`.
                .map(|(_, model, spec)| {
                    SessionConfigSelectOption::new(
                        format!("{}/{model}", config.provider),
                        spec.name.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !options
        .iter()
        .any(|option| option.value.to_string() == current)
    {
        options.push(SessionConfigSelectOption::new(
            current.clone(),
            config.model.clone(),
        ));
    }
    vec![
        SessionConfigOption::select(MODEL_CONFIG_ID, "Model", current, options)
            .category(SessionConfigOptionCategory::Model),
    ]
}

/// Build the agent selector advertised to ACP clients.
///
/// One value per available persona: the enabled built-ins first, then the
/// filesystem personas of the session's cwd and the XDG config directory.
/// `current` is the session's agent identity; when it is not among the
/// available personas it is added, so the client still renders a selector for
/// the active agent.
pub fn build_agent_config_options(
    agents_config: &AgentsConfig,
    cwd: &Path,
    agents_dir: &Path,
    current: Option<&str>,
) -> Vec<SessionConfigOption> {
    let resolver = FsPersonaResolver::new(
        cwd.to_path_buf(),
        agents_dir.to_path_buf(),
        agents_config.clone(),
    );
    let mut options: Vec<SessionConfigSelectOption> = resolver
        .list_available()
        .into_iter()
        .map(|summary| SessionConfigSelectOption::new(summary.name.clone(), summary.name))
        .collect();
    let current = current.unwrap_or_default();
    if !current.is_empty()
        && !options
            .iter()
            .any(|option| option.value.to_string() == current)
    {
        options.push(SessionConfigSelectOption::new(
            current.to_string(),
            current.to_string(),
        ));
    }
    vec![
        SessionConfigOption::select(AGENT_CONFIG_ID, "Agent", current.to_string(), options)
            .category(SessionConfigOptionCategory::Mode),
    ]
}

/// The `"provider/model"` identity of a resolved config.
fn model_value(config: &Config) -> String {
    format!("{}/{}", config.provider, config.model)
}
