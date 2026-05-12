use crate::providers::github_copilot::providers::contract::GitHubCopilotProvider;

#[test]
fn concrete_provider_owns_endpoint_and_mapping_openai4x() {
    assert_eq!(
        <super::OpenAI4xProvider as GitHubCopilotProvider>::ENDPOINT_PATH,
        "/chat/completions"
    );
    assert_eq!(
        <super::OpenAI4xProvider as GitHubCopilotProvider>::INTENT_HEADER,
        "conversation-agent"
    );
}
