//! Backend-specific implementations for GitHub Copilot API.
//!
//! GitHub Copilot is a proxy to multiple LLM backends (Anthropic, OpenAI).
//! Each backend requires a different `Openai-Intent` header value.
//!
//! This module provides a trait-based architecture with static dispatch
//! to select the correct backend at provider creation time, not request time.

/// Trait for GitHub Copilot backend implementations.
///
/// Each backend provides:
/// - The correct `Openai-Intent` header value for API requests
/// - A backend identifier for debugging/logging
///
/// # Static Dispatch
///
/// This trait is designed for static dispatch with generics:
/// ```rust,ignore
/// struct CompletionModel<B: GitHubCopilotBackend, H> {
///     backend: B,
///     // ...
/// }
/// ```
///
/// Never use `dyn GitHubCopilotBackend` - always use generics with trait bounds.
pub trait GitHubCopilotBackend: Default + Clone + Copy + Send + Sync {
    /// Returns the `Openai-Intent` header value for this backend.
    ///
    /// This is a static string that never changes at runtime.
    fn intent_header(&self) -> &'static str;

    /// Returns the backend name for debugging/logging.
    fn backend_name(&self) -> &'static str;
}

/// Anthropic backend for GitHub Copilot.
///
/// Used for Claude models (e.g., claude-sonnet-4.5).
/// Requires `Openai-Intent: conversation-panel` header.
#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicBackend;

impl GitHubCopilotBackend for AnthropicBackend {
    fn intent_header(&self) -> &'static str {
        "conversation-panel"
    }

    fn backend_name(&self) -> &'static str {
        "anthropic"
    }
}

/// OpenAI backend for GitHub Copilot.
///
/// Used for GPT models (e.g., gpt-4o, gpt-4o-mini, o1-preview, o1-mini).
/// Requires `Openai-Intent: conversation-agent` header.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAIBackend;

impl GitHubCopilotBackend for OpenAIBackend {
    fn intent_header(&self) -> &'static str {
        "conversation-agent"
    }

    fn backend_name(&self) -> &'static str {
        "openai"
    }
}

#[cfg(test)]
#[path = "backend_test.rs"]
mod tests;
