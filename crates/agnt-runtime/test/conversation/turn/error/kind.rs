use crate::conversation::turn::executor::{CompletionErrorCategory, CompletionErrorKind};

#[test]
fn is_retryable_matches_spec() {
    // Retryable kinds
    assert!(
        CompletionErrorKind::RateLimit.is_retryable(),
        "RateLimit must be retryable"
    );
    assert!(
        CompletionErrorKind::Overloaded.is_retryable(),
        "Overloaded must be retryable"
    );
    assert!(
        CompletionErrorKind::ServerError.is_retryable(),
        "ServerError must be retryable"
    );
    assert!(
        CompletionErrorKind::Network.is_retryable(),
        "Network must be retryable"
    );

    // Non-retryable kinds
    assert!(
        !CompletionErrorKind::RequestTooLarge.is_retryable(),
        "RequestTooLarge must not be retryable"
    );
    assert!(
        !CompletionErrorKind::ContextOverflow.is_retryable(),
        "ContextOverflow must not be retryable"
    );
    assert!(
        !CompletionErrorKind::ToolStructure.is_retryable(),
        "ToolStructure must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Auth.is_retryable(),
        "Auth must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Quota.is_retryable(),
        "Quota must not be retryable"
    );
    assert!(
        !CompletionErrorKind::CreditsExhausted.is_retryable(),
        "CreditsExhausted must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Refusal.is_retryable(),
        "Refusal must not be retryable"
    );
    assert!(
        !CompletionErrorKind::OutputBudget.is_retryable(),
        "OutputBudget must not be retryable"
    );
    assert!(
        !CompletionErrorKind::EndpointNotFound.is_retryable(),
        "EndpointNotFound must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Unknown.is_retryable(),
        "Unknown must not be retryable"
    );
}

/// Every one of the 14 variants maps to exactly one category from the single
/// classification site (`CompletionErrorKind::category`).
#[test]
fn category_maps_every_variant_to_exactly_one_category() {
    // -- Setup & Fixtures
    let all: &[(CompletionErrorKind, CompletionErrorCategory)] = &[
        (
            CompletionErrorKind::RateLimit,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::Overloaded,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::ServerError,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::Network,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::OutputBudget,
            CompletionErrorCategory::Steerable,
        ),
        (
            CompletionErrorKind::ToolStructure,
            CompletionErrorCategory::Steerable,
        ),
        (
            CompletionErrorKind::RequestTooLarge,
            CompletionErrorCategory::Steerable,
        ),
        (
            CompletionErrorKind::ContextOverflow,
            CompletionErrorCategory::HardStop,
        ),
        (CompletionErrorKind::Auth, CompletionErrorCategory::HardStop),
        (
            CompletionErrorKind::Quota,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::CreditsExhausted,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::Refusal,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::EndpointNotFound,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::Unknown,
            CompletionErrorCategory::HardStop,
        ),
    ];

    // -- Exec & Check
    for (kind, expected) in all {
        assert_eq!(
            kind.category(),
            *expected,
            "{kind:?} must map to {expected:?}"
        );
    }
}
