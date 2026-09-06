use super::*;

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Every one of the four model-correctable kinds is classified as
/// model-correctable.
#[test]
fn is_model_correctable_true_for_model_correctable_kinds() {
    // -- Setup & Fixtures
    let kinds = [
        CompletionErrorKind::ToolStructure,
        CompletionErrorKind::ContextOverflow,
        CompletionErrorKind::OutputBudget,
        CompletionErrorKind::RequestTooLarge,
    ];

    // -- Exec & Check
    for kind in &kinds {
        assert!(
            is_model_correctable(kind),
            "{kind:?} must be model-correctable"
        );
    }
}

/// Every remaining kind is classified as not model-correctable.
#[test]
fn is_model_correctable_false_for_non_model_correctable_kinds() {
    // -- Setup & Fixtures
    let kinds = [
        CompletionErrorKind::RateLimit,
        CompletionErrorKind::Overloaded,
        CompletionErrorKind::ServerError,
        CompletionErrorKind::Network,
        CompletionErrorKind::Auth,
        CompletionErrorKind::Quota,
        CompletionErrorKind::CreditsExhausted,
        CompletionErrorKind::Refusal,
        CompletionErrorKind::EndpointNotFound,
        CompletionErrorKind::Unknown,
    ];

    // -- Exec & Check
    for kind in &kinds {
        assert!(
            !is_model_correctable(kind),
            "{kind:?} must not be model-correctable"
        );
    }
}

/// The feedback message starts with the neutral prefix for every kind.
#[test]
fn build_feedback_message_starts_with_prefix_for_every_kind() {
    // -- Setup & Fixtures
    let kinds = [
        CompletionErrorKind::RateLimit,
        CompletionErrorKind::Overloaded,
        CompletionErrorKind::ServerError,
        CompletionErrorKind::Network,
        CompletionErrorKind::RequestTooLarge,
        CompletionErrorKind::ContextOverflow,
        CompletionErrorKind::OutputBudget,
        CompletionErrorKind::ToolStructure,
        CompletionErrorKind::Auth,
        CompletionErrorKind::Quota,
        CompletionErrorKind::CreditsExhausted,
        CompletionErrorKind::Refusal,
        CompletionErrorKind::EndpointNotFound,
        CompletionErrorKind::Unknown,
    ];

    // -- Exec & Check
    for kind in &kinds {
        let message = build_feedback_message(kind, "raw");
        assert!(
            message.starts_with(FEEDBACK_PREFIX),
            "message for {kind:?} must start with FEEDBACK_PREFIX, got: {message}"
        );
    }
}

/// Each model-correctable kind carries its pinned model-actionable remedy.
#[test]
fn build_feedback_message_contains_pinned_remedy_per_kind() -> TestResult<()> {
    // -- Setup & Fixtures
    let cases = [
        (
            CompletionErrorKind::ToolStructure,
            "ensure every tool call has a matching tool result",
        ),
        (
            CompletionErrorKind::ContextOverflow,
            "the conversation is too long; continue with a shorter request",
        ),
        (
            CompletionErrorKind::OutputBudget,
            "the response hit the output token limit; continue with a shorter response",
        ),
        (
            CompletionErrorKind::RequestTooLarge,
            "the request was too large; reduce the size of attached tool results",
        ),
    ];

    // -- Exec & Check
    for (kind, remedy) in &cases {
        let message = build_feedback_message(kind, "raw");
        assert!(
            message.contains(remedy),
            "message for {kind:?} must contain the pinned remedy, got: {message}"
        );
    }
    Ok(())
}

/// A raw provider message longer than the cap is cut to the first 500 bytes.
#[test]
fn build_feedback_message_truncates_raw_msg_at_500_bytes() {
    // -- Setup & Fixtures
    let raw_provider_msg = format!("{}{}", "a".repeat(500), "x".repeat(100));
    let kept = "a".repeat(500);
    let dropped = "x".repeat(100);

    // -- Exec & Check
    for kind in [
        CompletionErrorKind::ToolStructure,
        CompletionErrorKind::ContextOverflow,
        CompletionErrorKind::OutputBudget,
        CompletionErrorKind::RequestTooLarge,
    ] {
        let message = build_feedback_message(&kind, &raw_provider_msg);
        assert!(
            message.contains(&kept),
            "message for {kind:?} must keep the first 500 bytes"
        );
        assert!(
            !message.contains(&dropped),
            "message for {kind:?} must not contain bytes beyond the cap"
        );
        assert!(
            message.ends_with(&kept),
            "message for {kind:?} must end with the cut raw message"
        );
    }
}

/// A raw provider message shorter than the cap is kept verbatim.
#[test]
fn build_feedback_message_keeps_short_raw_msg_verbatim() {
    // -- Setup & Fixtures
    let raw_provider_msg = "context_length_exceeded: 12_345 tokens over";

    // -- Exec & Check
    for kind in [
        CompletionErrorKind::ToolStructure,
        CompletionErrorKind::ContextOverflow,
        CompletionErrorKind::OutputBudget,
        CompletionErrorKind::RequestTooLarge,
    ] {
        let message = build_feedback_message(&kind, raw_provider_msg);
        assert!(
            message.ends_with(raw_provider_msg),
            "message for {kind:?} must keep the short raw message verbatim, got: {message}"
        );
    }
}

/// The cut never splits a multi-byte UTF-8 character.
#[test]
fn build_feedback_message_cuts_at_char_boundary_for_multibyte_input() {
    // -- Setup & Fixtures
    // 499 ASCII bytes followed by multi-byte characters: byte 500 falls inside
    // the first 'é' (bytes 499-500), so the cut must fall back to byte 499.
    let raw_provider_msg = format!("{}{}", "a".repeat(499), "é".repeat(50));
    let kept = "a".repeat(499);

    // -- Exec & Check
    let message = build_feedback_message(&CompletionErrorKind::ContextOverflow, &raw_provider_msg);
    assert!(
        message.ends_with(&kept),
        "the cut must land on a valid char boundary, keeping the 499 ASCII bytes"
    );
}

/// The message never uses permission or escalation language: the remedy must
/// be model-actionable, not a user action.
#[test]
fn build_feedback_message_avoids_permission_language_for_empty_raw_msg() {
    // -- Setup & Fixtures
    let forbidden = ["permission", "grant", "escalat", "allow"];
    let kinds = [
        CompletionErrorKind::ToolStructure,
        CompletionErrorKind::ContextOverflow,
        CompletionErrorKind::OutputBudget,
        CompletionErrorKind::RequestTooLarge,
    ];

    // -- Exec & Check
    for kind in &kinds {
        let message = build_feedback_message(kind, "").to_lowercase();
        for word in &forbidden {
            assert!(
                !message.contains(word),
                "message for {kind:?} must not contain {word:?}, got: {message}"
            );
        }
    }
}
