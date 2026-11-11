#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ProviderKey {
    OpenAi,
    Anthropic,
    GitHubCopilot,
    Custom(String),
}

impl ProviderKey {
    pub(crate) fn from_input(provider: &str) -> Self {
        let normalized = provider.trim().to_lowercase();
        match normalized.as_str() {
            "openai" => Self::OpenAi,
            "anthropic" => Self::Anthropic,
            "github-copilot" => Self::GitHubCopilot,
            _ => Self::Custom(normalized),
        }
    }
}
