use super::factory::{Agent, agent_from_config};
use super::factory::{ProviderVariant, select_provider_variant};
use crate::providers::github_copilot::Error;

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
    assert!(matches!(err, Error::UnknownBackend(_)));
}

#[test]
fn invalid_model_format_returns_invalid_model_format_error() {
    let err = select_provider_variant("github-copilot", "gpt-4o").unwrap_err();
    assert!(matches!(err, Error::InvalidModelFormat(_)));
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

#[test]
fn agent_from_config_parses_backend_from_model() {
    let agent = agent_from_config(
        "github-copilot",
        "anthropic/claude-sonnet-4.5",
        Some("test-token".to_string()),
        Some("http://test".to_string()),
    )
    .unwrap();

    match agent {
        Agent::Anthropic(_) => {}
        _ => panic!("Expected Anthropic agent"),
    }

    let agent = agent_from_config(
        "github-copilot",
        "openai/gpt-4o",
        Some("test-token".to_string()),
        Some("http://test".to_string()),
    )
    .unwrap();

    match agent {
        Agent::OpenAI4x(_) => {}
        _ => panic!("Expected OpenAI 4x agent"),
    }

    let agent = agent_from_config(
        "github-copilot",
        "openai/gpt-5.3-codex",
        Some("test-token".to_string()),
        Some("http://test".to_string()),
    )
    .unwrap();

    match agent {
        Agent::OpenAI5x(_) => {}
        _ => panic!("Expected OpenAI 5x agent"),
    }
}

#[test]
fn agent_from_config_errors_on_invalid_model_format() {
    let result = agent_from_config(
        "github-copilot",
        "claude-sonnet-4.5",
        Some("test-token".to_string()),
        None,
    );

    assert!(result.is_err());
}

#[test]
fn error_message_shows_expected_format() {
    let result = agent_from_config(
        "github-copilot",
        "claude-sonnet-4.5",
        Some("test".to_string()),
        None,
    );

    assert!(result.is_err());
    if let Err(err) = result {
        let msg = err.to_string();
        assert!(
            msg.contains("backend/model"),
            "Error should mention expected format, got: {}",
            msg
        );
        assert!(
            msg.contains("anthropic/") || msg.contains("openai/"),
            "Error should show example, got: {}",
            msg
        );
        assert!(
            msg.contains("claude-sonnet-4.5"),
            "Error should show what was received, got: {}",
            msg
        );
    }
}

#[test]
fn error_message_shows_wrong_provider() {
    let result = agent_from_config(
        "github-copilot/anthropic",
        "anthropic/claude-sonnet-4.5",
        Some("test".to_string()),
        None,
    );

    assert!(result.is_err());
    if let Err(err) = result {
        let msg = err.to_string();
        assert!(
            msg.contains("github-copilot/anthropic"),
            "Error should show what was received, got: {}",
            msg
        );
        assert!(
            msg.contains("exactly"),
            "Error should emphasize exact match, got: {}",
            msg
        );
    }
}
