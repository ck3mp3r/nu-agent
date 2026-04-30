use crate::config::Config;
use sha2::{Digest, Sha256};
use std::fmt;

/// Stable identity key for cached providers.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProviderKey {
    pub provider: String,
    pub model: String,
    pub model_family: Option<String>,
    pub base_url: Option<String>,
    pub auth_fingerprint: Option<String>,
}

impl fmt::Debug for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderKey")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("model_family", &self.model_family)
            .field("base_url", &self.base_url)
            .field("auth_fingerprint", &self.auth_fingerprint)
            .finish()
    }
}

impl ProviderKey {
    pub fn from_config(config: &Config) -> Self {
        let provider = config.provider.clone();
        let auth = resolve_effective_auth(config);

        Self {
            provider,
            model: config.model.clone(),
            model_family: resolve_model_family(config),
            base_url: config.base_url.clone(),
            auth_fingerprint: auth.map(|s| auth_fingerprint(&s)),
        }
    }
}

pub fn auth_fingerprint(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();
    hex[..16].to_string()
}

fn resolve_effective_auth(config: &Config) -> Option<String> {
    if let Some(k) = &config.api_key {
        return Some(k.clone());
    }

    match config.provider.as_str() {
        "openai" => std::env::var("OPENAI_API_KEY").ok(),
        "anthropic" => std::env::var("ANTHROPIC_API_KEY").ok(),
        "github-copilot" => std::env::var("GITHUB_TOKEN").ok(),
        _ => None,
    }
}

fn resolve_model_family(config: &Config) -> Option<String> {
    if config.provider != "github-copilot" {
        return None;
    }

    let (backend, model_name) = config.model.split_once('/')?;
    match backend {
        "anthropic" => Some("anthropic".to_string()),
        "openai" if model_name.starts_with("gpt-5") => Some("openai5x".to_string()),
        "openai" => Some("openai4x".to_string()),
        _ => Some("unknown".to_string()),
    }
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
