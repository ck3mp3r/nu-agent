use super::*;
use serial_test::serial;

#[test]
fn test_resolve_model_name_with_backend_prefix() {
    // Legacy format with backend prefix
    let input = "anthropic/claude-sonnet-4.5";
    let expected = "claude-sonnet-4.5";
    assert_eq!(resolve_model_name(input), expected);
}

#[test]
fn test_resolve_model_name_without_prefix() {
    // New format without prefix
    let input = "claude-sonnet-4.5";
    let expected = "claude-sonnet-4.5";
    assert_eq!(resolve_model_name(input), expected);
}

#[test]
fn test_resolve_model_name_openai_prefix() {
    // OpenAI backend prefix
    let input = "openai/gpt-4o";
    let expected = "gpt-4o";
    assert_eq!(resolve_model_name(input), expected);
}

#[test]
fn test_resolve_model_name_codex() {
    // OpenAI codex model
    let input = "openai/gpt-5.3-codex";
    let expected = "gpt-5.3-codex";
    assert_eq!(resolve_model_name(input), expected);
}

#[test]
#[serial]
fn test_resolve_api_key_explicit() {
    // Explicit key takes precedence
    let explicit_key = Some("explicit_key".to_string());
    
    // Set env vars to verify explicit takes precedence
    unsafe {
        std::env::set_var("GITHUB_COPILOT_API_KEY", "env_copilot_key");
        std::env::set_var("GITHUB_TOKEN", "env_github_token");
    }
    
    let result = resolve_api_key(explicit_key);
    
    // Cleanup
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::remove_var("GITHUB_TOKEN");
    }
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "explicit_key");
}

#[test]
#[serial]
fn test_resolve_api_key_from_github_copilot_api_key() {
    // Falls back to GITHUB_COPILOT_API_KEY
    unsafe {
        std::env::set_var("GITHUB_COPILOT_API_KEY", "copilot_key");
        std::env::remove_var("GITHUB_TOKEN");
    }
    
    let result = resolve_api_key(None);
    
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
    }
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "copilot_key");
}

#[test]
#[serial]
fn test_resolve_api_key_from_github_token() {
    // Falls back to GITHUB_TOKEN if GITHUB_COPILOT_API_KEY not set
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::set_var("GITHUB_TOKEN", "github_token");
    }
    
    let result = resolve_api_key(None);
    
    unsafe {
        std::env::remove_var("GITHUB_TOKEN");
    }
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "github_token");
}

#[test]
#[serial]
fn test_resolve_api_key_missing() {
    // No key available - should return error
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::remove_var("GITHUB_TOKEN");
    }
    
    let result = resolve_api_key(None);
    
    assert!(result.is_err());
    match result {
        Err(CopilotError::MissingApiKey) => {
            // Expected error
        }
        _ => panic!("Expected MissingApiKey error"),
    }
}

#[test]
#[serial]
fn test_create_client_with_explicit_key() {
    // Should create client successfully with explicit key
    let api_key = Some("test_key".to_string());
    
    let result = create_client(api_key, None);
    
    assert!(result.is_ok());
}

#[test]
#[serial]
fn test_create_client_missing_key() {
    // Should fail without any key
    unsafe {
        std::env::remove_var("GITHUB_COPILOT_API_KEY");
        std::env::remove_var("GITHUB_TOKEN");
    }
    
    let result = create_client(None, None);
    
    assert!(result.is_err());
    match result {
        Err(CopilotError::MissingApiKey) => {
            // Expected
        }
        _ => panic!("Expected MissingApiKey error"),
    }
}
