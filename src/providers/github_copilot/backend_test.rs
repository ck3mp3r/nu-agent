use super::{AnthropicBackend, GitHubCopilotBackend, OpenAIBackend};

#[test]
fn anthropic_backend_returns_correct_intent() {
    let backend = AnthropicBackend;
    assert_eq!(backend.intent_header(), "conversation-panel");
}

#[test]
fn anthropic_backend_returns_correct_name() {
    let backend = AnthropicBackend;
    assert_eq!(backend.backend_name(), "anthropic");
}

#[test]
fn openai_backend_returns_correct_intent() {
    let backend = OpenAIBackend;
    assert_eq!(backend.intent_header(), "conversation-agent");
}

#[test]
fn openai_backend_returns_correct_name() {
    let backend = OpenAIBackend;
    assert_eq!(backend.backend_name(), "openai");
}

#[test]
fn backends_implement_default() {
    // Verify backends implement Default trait (required for static dispatch)
    let _anthropic = AnthropicBackend::default();
    let _openai = OpenAIBackend::default();
}

#[test]
fn backends_are_copy_and_clone() {
    // Verify backends are Copy and Clone (zero-cost abstractions)
    let anthropic = AnthropicBackend;
    let _anthropic2 = anthropic; // Copy
    let _anthropic3 = anthropic.clone(); // Clone

    let openai = OpenAIBackend;
    let _openai2 = openai;
    let _openai3 = openai.clone();
}
