use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;

#[test]
fn concrete_provider_owns_endpoint_and_mapping_anthropic() {
    assert_eq!(
        <super::AnthropicProvider as GitHubCopilotProvider>::ENDPOINT_PATH,
        "/chat/completions"
    );
    assert_eq!(
        <super::AnthropicProvider as GitHubCopilotProvider>::INTENT_HEADER,
        "conversation-panel"
    );
}
