use super::factory::{ProviderVariant, select_provider_variant};

#[test]
fn routes_anthropic_models_to_anthropic_chat_variant() {
    let variant = select_provider_variant("github-copilot", "anthropic/claude-sonnet-4.5").unwrap();
    assert!(matches!(variant, ProviderVariant::Anthropic));
}

#[test]
fn routes_openai_4x_models_to_openai_4x_chat_variant() {
    let gpt4o = select_provider_variant("github-copilot", "openai/gpt-4o").unwrap();
    assert!(matches!(gpt4o, ProviderVariant::OpenAI4x));

    let gpt4o_mini = select_provider_variant("github-copilot", "openai/gpt-4o-mini").unwrap();
    assert!(matches!(gpt4o_mini, ProviderVariant::OpenAI4x));

    let gpt41 = select_provider_variant("github-copilot", "openai/gpt-4.1").unwrap();
    assert!(matches!(gpt41, ProviderVariant::OpenAI4x));
}

#[test]
fn routes_openai_5x_models_to_openai_5x_responses_variant() {
    let gpt5 = select_provider_variant("github-copilot", "openai/gpt-5").unwrap();
    assert!(matches!(gpt5, ProviderVariant::OpenAI5x));

    let gpt5mini = select_provider_variant("github-copilot", "openai/gpt-5-mini").unwrap();
    assert!(matches!(gpt5mini, ProviderVariant::OpenAI5x));

    let gpt53 = select_provider_variant("github-copilot", "openai/gpt-5.3-codex").unwrap();
    assert!(matches!(gpt53, ProviderVariant::OpenAI5x));

    let gpt5codex = select_provider_variant("github-copilot", "openai/gpt-5-codex").unwrap();
    assert!(matches!(gpt5codex, ProviderVariant::OpenAI5x));
}

#[test]
fn unknown_backend_returns_unknown_backend_error() {
    let err = select_provider_variant("github-copilot", "foobar/some-model").unwrap_err();
    assert!(matches!(err, super::Error::UnknownBackend(_)));
}

#[test]
fn invalid_model_format_returns_invalid_model_format_error() {
    let err = select_provider_variant("github-copilot", "gpt-4o").unwrap_err();
    assert!(matches!(err, super::Error::InvalidModelFormat(_)));
}

#[test]
fn factory_selects_concrete_provider_once() {
    let anthropic =
        select_provider_variant("github-copilot", "anthropic/claude-sonnet-4.5").unwrap();
    assert!(matches!(anthropic, ProviderVariant::Anthropic));

    let openai4x = select_provider_variant("github-copilot", "openai/gpt-4o-mini").unwrap();
    assert!(matches!(openai4x, ProviderVariant::OpenAI4x));

    let openai5x = select_provider_variant("github-copilot", "openai/gpt-5.3-codex").unwrap();
    assert!(matches!(openai5x, ProviderVariant::OpenAI5x));
}

#[test]
fn no_shared_endpoint_path_switch_helpers() {
    assert_eq!(
        <crate::providers::github_copilot::providers::AnthropicProvider as crate::providers::github_copilot::providers::contract::GitHubCopilotProvider>::ENDPOINT_PATH,
        "/chat/completions"
    );
    assert_eq!(
        <crate::providers::github_copilot::providers::OpenAI4xProvider as crate::providers::github_copilot::providers::contract::GitHubCopilotProvider>::ENDPOINT_PATH,
        "/chat/completions"
    );
    assert_eq!(
        <crate::providers::github_copilot::providers::OpenAI5xProvider as crate::providers::github_copilot::providers::contract::GitHubCopilotProvider>::ENDPOINT_PATH,
        "/responses"
    );
}
