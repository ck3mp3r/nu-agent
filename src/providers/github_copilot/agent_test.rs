//! Tests for Agent factory

use super::Agent;

#[test]
fn agent_from_config_parses_backend_from_model() {
    // Test anthropic backend - expects model string to contain backend
    let agent = Agent::from_config(
        "github-copilot",              // Provider without backend
        "anthropic/claude-sonnet-4.5", // Backend in model
        Some("test-token".to_string()),
        Some("http://test".to_string()),
    )
    .unwrap();

    match agent {
        Agent::Anthropic(_) => {}
        _ => panic!("Expected Anthropic agent"),
    }

    // Test openai backend
    let agent = Agent::from_config(
        "github-copilot",
        "openai/gpt-4o",
        Some("test-token".to_string()),
        Some("http://test".to_string()),
    )
    .unwrap();

    match agent {
        Agent::OpenAI(_) => {}
        _ => panic!("Expected OpenAI agent"),
    }
}

#[test]
fn agent_from_config_errors_on_invalid_model_format() {
    let result = Agent::from_config(
        "github-copilot",
        "claude-sonnet-4.5", // Missing backend prefix
        Some("test-token".to_string()),
        None,
    );

    assert!(result.is_err());
}

#[test]
fn error_message_shows_expected_format() {
    let result = Agent::from_config(
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
    let result = Agent::from_config(
        "github-copilot/anthropic", // Old format
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
