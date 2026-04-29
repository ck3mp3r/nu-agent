use crate::llm::{LlmResponse, extract_response};
use crate::providers::github_copilot;
use crate::providers::github_copilot::providers::{
    AnthropicProvider, OpenAI4xProvider, OpenAI5xProvider,
};
use nu_protocol::LabeledError;
use rig::client::CompletionClient;

/// Concrete configured provider variants held in runtime cache.
pub enum CachedProvider {
    OpenAi {
        client: rig::providers::openai::Client,
        model: String,
    },
    Anthropic {
        client: rig::providers::anthropic::Client,
        model: String,
    },
    Ollama {
        client: rig::providers::ollama::Client,
        model: String,
    },
    GitHubCopilotAnthropic {
        client: github_copilot::Client,
        model: String,
    },
    GitHubCopilotOpenAI4x {
        client: github_copilot::Client,
        model: String,
    },
    GitHubCopilotOpenAI5x {
        client: github_copilot::Client,
        model: String,
    },
    #[cfg(test)]
    TestTag(u64),
}

pub async fn execute(
    provider: &CachedProvider,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError> {
    match provider {
        CachedProvider::OpenAi { client, model } => {
            let agent = client.agent(model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        CachedProvider::Anthropic { client, model } => {
            let agent = client.agent(model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        CachedProvider::Ollama { client, model } => {
            let agent = client.agent(model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        CachedProvider::GitHubCopilotAnthropic { client, model } => {
            let completion_model = github_copilot::completion::CompletionModel::<
                AnthropicProvider,
                _,
            >::new(client.clone(), model);
            let agent = rig::agent::AgentBuilder::new(completion_model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        CachedProvider::GitHubCopilotOpenAI4x { client, model } => {
            let completion_model =
                github_copilot::completion::CompletionModel::<OpenAI4xProvider, _>::new(
                    client.clone(),
                    model,
                );
            let agent = rig::agent::AgentBuilder::new(completion_model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        CachedProvider::GitHubCopilotOpenAI5x { client, model } => {
            let completion_model =
                github_copilot::completion::CompletionModel::<OpenAI5xProvider, _>::new(
                    client.clone(),
                    model,
                );
            let agent = rig::agent::AgentBuilder::new(completion_model).build();
            call_agent_completion(agent, prompt, tools).await
        }
        #[cfg(test)]
        CachedProvider::TestTag(_) => Err(LabeledError::new(
            "Test provider cannot execute completion requests",
        )),
    }
}

async fn call_agent_completion<M, A>(
    agent: A,
    prompt: &str,
    tools: Vec<rig::completion::ToolDefinition>,
) -> Result<LlmResponse, LabeledError>
where
    M: rig::completion::CompletionModel,
    A: rig::completion::Completion<M>,
{
    let response = agent
        .completion(prompt, Vec::<rig::completion::Message>::new())
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?
        .tools(tools)
        .send()
        .await
        .map_err(|e| LabeledError::new(format!("LLM call failed: {}", e)))?;

    extract_response(response)
}
