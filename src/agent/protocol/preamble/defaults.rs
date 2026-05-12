use std::collections::HashMap;

use super::ModelFamily;

const GITHUB_COPILOT_OPENAI_GPT5X_DEFAULT: &str =
    include_str!("defaults/github_copilot_openai_gpt5x.md");
const GITHUB_COPILOT_OPENAI_GPT4X_DEFAULT: &str =
    include_str!("defaults/github_copilot_openai_gpt4x.md");
const GITHUB_COPILOT_ANTHROPIC_DEFAULT: &str = include_str!("defaults/github_copilot_anthropic.md");
const OPENAI_GPT5X_DEFAULT: &str = include_str!("defaults/openai_gpt5x.md");
const OPENAI_GPT4X_DEFAULT: &str = include_str!("defaults/openai_gpt4x.md");
const ANTHROPIC_DEFAULT: &str = include_str!("defaults/anthropic.md");
const GLOBAL_FALLBACK_DEFAULT: &str = include_str!("defaults/global_fallback.md");

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ProviderKey {
    OpenAi,
    Anthropic,
    GitHubCopilot,
    Custom(String),
}

impl ProviderKey {
    fn from_input(provider: &str) -> Self {
        let normalized = provider.trim().to_lowercase();
        match normalized.as_str() {
            "openai" => Self::OpenAi,
            "anthropic" => Self::Anthropic,
            "github-copilot" => Self::GitHubCopilot,
            _ => Self::Custom(normalized),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreambleDefaults {
    provider: HashMap<ProviderKey, String>,
    provider_family: HashMap<(ProviderKey, ModelFamily), String>,
    global_fallback: Option<String>,
}

impl PreambleDefaults {
    pub fn builtin() -> Self {
        let mut provider = HashMap::new();
        provider.insert(ProviderKey::Anthropic, ANTHROPIC_DEFAULT.trim().to_string());

        let mut provider_family = HashMap::new();
        provider_family.insert(
            (ProviderKey::GitHubCopilot, ModelFamily::Gpt5x),
            GITHUB_COPILOT_OPENAI_GPT5X_DEFAULT.trim().to_string(),
        );
        provider_family.insert(
            (ProviderKey::GitHubCopilot, ModelFamily::Gpt4x),
            GITHUB_COPILOT_OPENAI_GPT4X_DEFAULT.trim().to_string(),
        );
        provider_family.insert(
            (ProviderKey::GitHubCopilot, ModelFamily::Anthropic),
            GITHUB_COPILOT_ANTHROPIC_DEFAULT.trim().to_string(),
        );
        provider_family.insert(
            (ProviderKey::OpenAi, ModelFamily::Gpt5x),
            OPENAI_GPT5X_DEFAULT.trim().to_string(),
        );
        provider_family.insert(
            (ProviderKey::OpenAi, ModelFamily::Gpt4x),
            OPENAI_GPT4X_DEFAULT.trim().to_string(),
        );

        Self {
            provider,
            provider_family,
            global_fallback: Some(GLOBAL_FALLBACK_DEFAULT.trim().to_string()),
        }
    }

    pub fn set_provider_preamble(&mut self, provider: &str, preamble: impl Into<String>) {
        self.provider
            .insert(ProviderKey::from_input(provider), preamble.into());
    }

    pub fn set_provider_family_preamble(
        &mut self,
        provider: &str,
        family: ModelFamily,
        preamble: impl Into<String>,
    ) {
        self.provider_family
            .insert((ProviderKey::from_input(provider), family), preamble.into());
    }

    pub fn set_global_fallback(&mut self, preamble: Option<String>) {
        self.global_fallback = preamble;
    }

    pub fn provider_preamble(&self, provider: &str) -> Option<&str> {
        self.provider
            .get(&ProviderKey::from_input(provider))
            .map(String::as_str)
    }

    pub fn provider_family_preamble(&self, provider: &str, family: ModelFamily) -> Option<&str> {
        self.provider_family
            .get(&(ProviderKey::from_input(provider), family))
            .map(String::as_str)
    }

    pub fn global_fallback(&self) -> Option<&str> {
        self.global_fallback.as_deref()
    }
}

impl Default for PreambleDefaults {
    fn default() -> Self {
        Self::builtin()
    }
}
