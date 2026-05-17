use crate::providers::github_copilot::completion::CompletionModel;
use crate::providers::github_copilot::providers::{
    AnthropicProvider, OpenAI4xProvider, OpenAI5xProvider,
};
use crate::providers::github_copilot::{Client, Error};
use rig::completion::Completion;

/// GitHub Copilot agent variants selected once from model family.
pub enum Agent<H = reqwest::Client>
where
    H: rig::http_client::HttpClientExt + Default + std::fmt::Debug + Clone + 'static,
{
    Anthropic(
        rig::agent::Agent<CompletionModel<AnthropicProvider, H>>,
        Client<H>,
        String,
    ),
    OpenAI4x(
        rig::agent::Agent<CompletionModel<OpenAI4xProvider, H>>,
        Client<H>,
        String,
    ),
    OpenAI5x(
        rig::agent::Agent<CompletionModel<OpenAI5xProvider, H>>,
        Client<H>,
        String,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderVariant {
    Anthropic,
    OpenAI4x,
    OpenAI5x,
}

impl ProviderVariant {
    pub fn from_provider_model(provider_string: &str, model: &str) -> Result<Self, Error> {
        if provider_string != "github-copilot" {
            return Err(Error::InvalidProviderFormat(provider_string.to_string()));
        }

        let (backend, model_name) = model
            .split_once('/')
            .ok_or_else(|| Error::InvalidModelFormat(model.to_string()))?;

        match backend {
            "anthropic" => Ok(Self::Anthropic),
            "openai" if model_name.starts_with("gpt-5") => Ok(Self::OpenAI5x),
            "openai" => Ok(Self::OpenAI4x),
            _ => Err(Error::UnknownBackend(backend.to_string())),
        }
    }
}

fn resolve_api_key(api_key: Option<String>) -> Result<String, Error> {
    api_key
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .ok_or(Error::MissingApiKey)
}

pub fn agent_from_config(
    provider_string: &str,
    model: &str,
    api_key: Option<String>,
    base_url: Option<String>,
) -> Result<Agent, Error> {
    let variant = ProviderVariant::from_provider_model(provider_string, model)?;

    let (_, model_name) = model
        .split_once('/')
        .ok_or_else(|| Error::InvalidModelFormat(model.to_string()))?;

    let key = resolve_api_key(api_key)?;

    let client = if let Some(url) = base_url {
        Client::builder().api_key(key).base_url(url).build()?
    } else {
        Client::builder().api_key(key).build()?
    };

    let agent = match variant {
        ProviderVariant::Anthropic => {
            let model = CompletionModel::<AnthropicProvider, _>::new(client.clone(), model_name);
            Agent::Anthropic(
                rig::agent::AgentBuilder::new(model).build(),
                client,
                model_name.to_string(),
            )
        }
        ProviderVariant::OpenAI4x => {
            let model = CompletionModel::<OpenAI4xProvider, _>::new(client.clone(), model_name);
            Agent::OpenAI4x(
                rig::agent::AgentBuilder::new(model).build(),
                client,
                model_name.to_string(),
            )
        }
        ProviderVariant::OpenAI5x => {
            let model = CompletionModel::<OpenAI5xProvider, _>::new(client.clone(), model_name);
            Agent::OpenAI5x(
                rig::agent::AgentBuilder::new(model).build(),
                client,
                model_name.to_string(),
            )
        }
    };

    Ok(agent)
}

impl<H> Agent<H>
where
    H: rig::http_client::HttpClientExt + Default + std::fmt::Debug + Clone + 'static,
{
    /// Execute a single-turn completion with the given prompt text.
    ///
    /// This is a simpler interface than the full multi-turn agent.prompt() loop.
    /// Use for one-off completions without tool calls or chat history.
    pub async fn completion(
        &self,
        prompt_text: &str,
    ) -> Result<String, rig::completion::CompletionError> {
        use rig::completion::AssistantContent;

        let extract_text = |response: rig::completion::CompletionResponse<_>| {
            let mut text_parts = Vec::new();
            for content in response.choice {
                if let AssistantContent::Text(t) = content {
                    text_parts.push(t.to_string());
                }
            }
            text_parts.join("\n")
        };

        match self {
            Agent::Anthropic(agent, ..) => {
                let response = agent
                    .completion(prompt_text, Vec::<rig::completion::Message>::new())
                    .await?
                    .tools(vec![])
                    .send()
                    .await?;
                Ok(extract_text(response))
            }
            Agent::OpenAI4x(agent, ..) => {
                let response = agent
                    .completion(prompt_text, Vec::<rig::completion::Message>::new())
                    .await?
                    .tools(vec![])
                    .send()
                    .await?;
                Ok(extract_text(response))
            }
            Agent::OpenAI5x(agent, ..) => {
                let response = agent
                    .completion(prompt_text, Vec::<rig::completion::Message>::new())
                    .await?
                    .tools(vec![])
                    .send()
                    .await?;
                Ok(extract_text(response))
            }
        }
    }
}

#[cfg(test)]
mod test;
