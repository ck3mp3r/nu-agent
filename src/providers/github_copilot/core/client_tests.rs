use super::ClientExt;
use super::*;

#[test]
#[serial_test::serial]
fn agent_from_config_parses_anthropic_backend() {
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "test-token");
    }
    let result =
        Client::agent_from_config("github-copilot", "anthropic/claude-sonnet-4.5", None, None);
    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }
    assert!(result.is_ok());
}

#[test]
#[serial_test::serial]
fn agent_from_config_parses_openai_backend() {
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "test-token");
    }
    let result = Client::agent_from_config("github-copilot", "openai/gpt-4o", None, None);
    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }
    assert!(result.is_ok());
}

#[test]
#[serial_test::serial]
fn agent_from_config_uses_provided_api_key() {
    let result = Client::agent_from_config(
        "github-copilot",
        "openai/gpt-4o",
        Some("custom-key".to_string()),
        None,
    );
    assert!(result.is_ok());
}

#[test]
#[serial_test::serial]
fn agent_from_config_fails_invalid_format() {
    let result = Client::agent_from_config(
        "github-copilot",
        "model",
        Some("key".to_string()),
        None,
    );
    assert!(result.is_err());
}

#[test]
#[serial_test::serial]
fn agent_from_config_fails_unknown_backend() {
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "test-token");
    }
    let result = Client::agent_from_config("github-copilot", "unknown/model", None, None);
    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }
    assert!(matches!(
        result,
        Err(crate::providers::github_copilot::Error::UnknownBackend(_))
    ));
}

#[test]
#[serial_test::serial]
fn agent_from_config_fails_missing_api_key() {
    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }
    let result = Client::agent_from_config("github-copilot", "openai/model", None, None);
    assert!(matches!(
        result,
        Err(crate::providers::github_copilot::Error::MissingApiKey)
    ));
}
