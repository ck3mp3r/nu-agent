use super::providers::{AnthropicProvider, OpenAI4xProvider, OpenAI5xProvider};
use super::{Agent, Error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderVariant {
    Anthropic,
    OpenAI4x,
    OpenAI5x,
}

fn resolve_api_key(api_key: Option<String>) -> Result<String, Error> {
    api_key
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .ok_or(Error::MissingApiKey)
}

pub fn is_openai_5x_model(model_name: &str) -> bool {
    model_name.starts_with("gpt-5")
}

pub fn select_provider_variant(
    provider_string: &str,
    model: &str,
) -> Result<ProviderVariant, Error> {
    if provider_string != "github-copilot" {
        return Err(Error::InvalidProviderFormat(provider_string.to_string()));
    }

    let (backend, model_name) = model
        .split_once('/')
        .ok_or_else(|| Error::InvalidModelFormat(model.to_string()))?;

    match backend {
        "anthropic" => Ok(ProviderVariant::Anthropic),
        "openai" if is_openai_5x_model(model_name) => Ok(ProviderVariant::OpenAI5x),
        "openai" => Ok(ProviderVariant::OpenAI4x),
        _ => Err(Error::UnknownBackend(backend.to_string())),
    }
}

pub fn agent_from_config(
    provider_string: &str,
    model: &str,
    api_key: Option<String>,
    base_url: Option<String>,
) -> Result<Agent, Error> {
    let variant = select_provider_variant(provider_string, model)?;

    let (_, model_name) = model
        .split_once('/')
        .ok_or_else(|| Error::InvalidModelFormat(model.to_string()))?;

    let key = resolve_api_key(api_key)?;

    let client = if let Some(url) = base_url {
        super::Client::builder()
            .api_key(key)
            .base_url(url)
            .build()?
    } else {
        super::Client::builder().api_key(key).build()?
    };

    let agent = match variant {
        ProviderVariant::Anthropic => {
            let model =
                super::completion::CompletionModel::<AnthropicProvider, _>::new(client, model_name);
            Agent::Anthropic(rig::agent::AgentBuilder::new(model).build())
        }
        ProviderVariant::OpenAI4x => {
            let model =
                super::completion::CompletionModel::<OpenAI4xProvider, _>::new(client, model_name);
            Agent::OpenAI4x(rig::agent::AgentBuilder::new(model).build())
        }
        ProviderVariant::OpenAI5x => {
            let model =
                super::completion::CompletionModel::<OpenAI5xProvider, _>::new(client, model_name);
            Agent::OpenAI5x(rig::agent::AgentBuilder::new(model).build())
        }
    };

    Ok(agent)
}
