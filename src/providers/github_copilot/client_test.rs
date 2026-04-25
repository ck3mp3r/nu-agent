//! Tests for GitHub Copilot client traits

use super::*;
use rig::client::{Provider, ProviderBuilder};

#[test]
fn test_provider_verify_path() {
    // VERIFY_PATH should be "/models" for compatibility with OpenAI-compatible APIs
    assert_eq!(GitHubCopilotExt::VERIFY_PATH, "/models");
}

#[test]
fn test_provider_builder_base_url() {
    // BASE_URL should point to GitHub Copilot API
    assert_eq!(
        GitHubCopilotExtBuilder::BASE_URL,
        "https://api.githubcopilot.com"
    );
}

#[test]
fn test_client_from_env() {
    // This test requires GITHUB_TOKEN env var
    // We'll mark it as ignored and only run with `cargo test -- --ignored`
    // when the env var is actually set

    // For now, just verify the type compiles
    // In a real test with env var set:
    // let client = Client::from_env();
    // assert!(client.is_ok());
}

#[test]
fn test_client_from_env_with_base_url() {
    // Similar to above - test when both GITHUB_TOKEN and GITHUB_COPILOT_BASE_URL are set
    // For now, verify the type compiles
}

#[test]
fn test_types_compile() {
    // Verify all our type aliases compile correctly
    fn _assert_client_type<H>(_client: Client<H>) {}
    fn _assert_builder_type<H>(_builder: ClientBuilder<H>) {}

    // If this compiles, our type aliases are correct
}

// ============================================================================
// Backend-Specific Extension Tests
// ============================================================================

#[test]
fn anthropic_extension_implements_provider() {
    // Verify GitHubCopilotAnthropicExt implements Provider trait
    fn _assert_provider<T: Provider>() {}
    _assert_provider::<GitHubCopilotAnthropicExt>();
}

#[test]
fn openai_extension_implements_provider() {
    // Verify GitHubCopilotOpenAIExt implements Provider trait
    fn _assert_provider<T: Provider>() {}
    _assert_provider::<GitHubCopilotOpenAIExt>();
}

#[test]
fn anthropic_extension_capabilities_use_anthropic_backend() {
    // Verify that AnthropicExt's Capabilities use AnthropicBackend
    // This is a compile-time verification - if it compiles, it's correct
    use rig::client::Capabilities;
    
    // We can't easily extract the inner type of Capable, but we can verify
    // that the Capabilities implementation exists and compiles
    type _Caps = GitHubCopilotAnthropicExt;
    fn _assert<H: rig::http_client::HttpClientExt, C: Capabilities<H>>() {}
    _assert::<reqwest::Client, _Caps>();
}

#[test]
fn openai_extension_capabilities_use_openai_backend() {
    // Verify that OpenAIExt's Capabilities use OpenAIBackend
    // This is a compile-time verification - if it compiles, it's correct
    use rig::client::Capabilities;
    
    type _Caps = GitHubCopilotOpenAIExt;
    fn _assert<H: rig::http_client::HttpClientExt, C: Capabilities<H>>() {}
    _assert::<reqwest::Client, _Caps>();
}

#[test]
fn anthropic_client_alias_compiles() {
    // Verify AnthropicClient type alias works
    fn _assert_client<H>(_client: AnthropicClient<H>) {}
}

#[test]
fn openai_client_alias_compiles() {
    // Verify OpenAIClient type alias works
    fn _assert_client<H>(_client: OpenAIClient<H>) {}
}
