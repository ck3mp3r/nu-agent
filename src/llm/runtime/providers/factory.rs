use super::cached::CachedProvider;
use crate::config::Config;
use crate::providers::github_copilot;
use nu_protocol::LabeledError;

pub fn build_cached_provider(config: &Config) -> Result<CachedProvider, LabeledError> {
    match config.provider.as_str() {
        "openai" => {
            let key = resolve_auth(config, "OPENAI_API_KEY")?;
            let client = if let Some(url) = &config.base_url {
                rig::providers::openai::Client::builder()
                    .api_key(key)
                    .base_url(url.clone())
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create OpenAI client: {}", e))
                    })?
            } else {
                rig::providers::openai::Client::builder()
                    .api_key(key)
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create OpenAI client: {}", e))
                    })?
            };

            Ok(CachedProvider::OpenAi {
                client,
                model: config.model.clone(),
            })
        }
        "anthropic" => {
            let key = resolve_auth(config, "ANTHROPIC_API_KEY")?;
            let client = if let Some(url) = &config.base_url {
                rig::providers::anthropic::Client::builder()
                    .api_key(key)
                    .base_url(url.clone())
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create Anthropic client: {}", e))
                    })?
            } else {
                rig::providers::anthropic::Client::builder()
                    .api_key(key)
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create Anthropic client: {}", e))
                    })?
            };

            Ok(CachedProvider::Anthropic {
                client,
                model: config.model.clone(),
            })
        }
        "ollama" => {
            let url = config
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:11434");
            let client = rig::providers::ollama::Client::builder()
                .api_key(rig::client::Nothing)
                .base_url(url)
                .build()
                .map_err(|e| LabeledError::new(format!("Failed to create Ollama client: {}", e)))?;

            Ok(CachedProvider::Ollama {
                client,
                model: config.model.clone(),
            })
        }
        "github-copilot" => {
            let variant = github_copilot::model::factory::select_provider_variant(
                "github-copilot",
                &config.model,
            )
            .map_err(|e| {
                LabeledError::new(format!("Failed to select GitHub Copilot provider: {}", e))
            })?;
            let key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("GITHUB_TOKEN").ok())
                .ok_or_else(|| LabeledError::new("Missing GITHUB_TOKEN"))?;

            let client = if let Some(url) = &config.base_url {
                github_copilot::Client::builder()
                    .api_key(key)
                    .base_url(url.clone())
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create GitHub Copilot client: {}", e))
                    })?
            } else {
                github_copilot::Client::builder()
                    .api_key(key)
                    .build()
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to create GitHub Copilot client: {}", e))
                    })?
            };

            let (_, model_name) = config.model.split_once('/').ok_or_else(|| {
                LabeledError::new(format!(
                    "Failed to create GitHub Copilot agent: Invalid model format: {}",
                    config.model
                ))
            })?;

            match variant {
                github_copilot::model::factory::ProviderVariant::Anthropic => {
                    Ok(CachedProvider::GitHubCopilotAnthropic {
                        client,
                        model: model_name.to_string(),
                    })
                }
                github_copilot::model::factory::ProviderVariant::OpenAI4x => {
                    Ok(CachedProvider::GitHubCopilotOpenAI4x {
                        client,
                        model: model_name.to_string(),
                    })
                }
                github_copilot::model::factory::ProviderVariant::OpenAI5x => {
                    Ok(CachedProvider::GitHubCopilotOpenAI5x {
                        client,
                        model: model_name.to_string(),
                    })
                }
            }
        }
        other => Err(LabeledError::new(format!(
            "Unsupported provider: {}",
            other
        ))),
    }
}

fn resolve_auth(config: &Config, env_key: &str) -> Result<String, LabeledError> {
    config
        .api_key
        .clone()
        .or_else(|| std::env::var(env_key).ok())
        .ok_or_else(|| LabeledError::new(format!("Missing {}", env_key)))
}
