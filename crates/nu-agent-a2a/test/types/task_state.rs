use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// TaskState
// ---------------------------------------------------------------------------

#[test]
fn task_state_all_variants_serde() {
    use serde_test::{Token, assert_tokens};

    let cases: &[(TaskState, &str)] = &[
        (TaskState::Unspecified, "TASK_STATE_UNSPECIFIED"),
        (TaskState::Submitted, "TASK_STATE_SUBMITTED"),
        (TaskState::Working, "TASK_STATE_WORKING"),
        (TaskState::InputRequired, "TASK_STATE_INPUT_REQUIRED"),
        (TaskState::Completed, "TASK_STATE_COMPLETED"),
        (TaskState::Failed, "TASK_STATE_FAILED"),
        (TaskState::Canceled, "TASK_STATE_CANCELED"),
        (TaskState::Rejected, "TASK_STATE_REJECTED"),
        (TaskState::AuthRequired, "TASK_STATE_AUTH_REQUIRED"),
    ];

    for (variant, expected) in cases {
        assert_tokens(
            variant,
            &[Token::UnitVariant {
                name: "TaskState",
                variant: expected,
            }],
        );
    }
}

#[test]
fn task_state_unknown_string_fails_deserialize() {
    let result: core::result::Result<TaskState, _> = serde_json::from_str("\"unknown_state\"");
    assert!(result.is_err());
}

#[test]
fn task_state_legacy_string_fails_deserialize() {
    let result: core::result::Result<TaskState, _> = serde_json::from_str("\"working\"");
    assert!(result.is_err(), "legacy 'working' must not deserialize");
}

#[test]
fn task_state_display_output() {
    assert_eq!(TaskState::Unspecified.to_string(), "TASK_STATE_UNSPECIFIED");
    assert_eq!(TaskState::Submitted.to_string(), "TASK_STATE_SUBMITTED");
    assert_eq!(TaskState::Working.to_string(), "TASK_STATE_WORKING");
    assert_eq!(
        TaskState::InputRequired.to_string(),
        "TASK_STATE_INPUT_REQUIRED"
    );
    assert_eq!(TaskState::Completed.to_string(), "TASK_STATE_COMPLETED");
    assert_eq!(TaskState::Failed.to_string(), "TASK_STATE_FAILED");
    assert_eq!(TaskState::Canceled.to_string(), "TASK_STATE_CANCELED");
    assert_eq!(TaskState::Rejected.to_string(), "TASK_STATE_REJECTED");
    assert_eq!(
        TaskState::AuthRequired.to_string(),
        "TASK_STATE_AUTH_REQUIRED"
    );
}

#[test]
fn task_state_label_output() {
    assert_eq!(TaskState::Unspecified.label(), "unspecified");
    assert_eq!(TaskState::Submitted.label(), "submitted");
    assert_eq!(TaskState::Working.label(), "working");
    assert_eq!(TaskState::InputRequired.label(), "input-required");
    assert_eq!(TaskState::Completed.label(), "completed");
    assert_eq!(TaskState::Failed.label(), "failed");
    assert_eq!(TaskState::Canceled.label(), "canceled");
    assert_eq!(TaskState::Rejected.label(), "rejected");
    assert_eq!(TaskState::AuthRequired.label(), "auth-required");
}

#[test]
fn task_state_is_terminal() {
    assert!(TaskState::Completed.is_terminal());
    assert!(TaskState::Failed.is_terminal());
    assert!(TaskState::Canceled.is_terminal());
    assert!(TaskState::Rejected.is_terminal());
    assert!(!TaskState::Working.is_terminal());
    assert!(!TaskState::Submitted.is_terminal());
}

#[test]
fn task_state_deserialize_spec_format() -> Result<()> {
    let state: TaskState = serde_json::from_str("\"TASK_STATE_WORKING\"")?;
    assert_eq!(state, TaskState::Working);

    let state: TaskState = serde_json::from_str("\"TASK_STATE_AUTH_REQUIRED\"")?;
    assert_eq!(state, TaskState::AuthRequired);
    Ok(())
}

#[test]
fn task_state_try_from_spec_format() -> Result<()> {
    let cases: &[(&str, TaskState)] = &[
        ("TASK_STATE_UNSPECIFIED", TaskState::Unspecified),
        ("TASK_STATE_SUBMITTED", TaskState::Submitted),
        ("TASK_STATE_WORKING", TaskState::Working),
        ("TASK_STATE_INPUT_REQUIRED", TaskState::InputRequired),
        ("TASK_STATE_COMPLETED", TaskState::Completed),
        ("TASK_STATE_FAILED", TaskState::Failed),
        ("TASK_STATE_CANCELED", TaskState::Canceled),
        ("TASK_STATE_REJECTED", TaskState::Rejected),
        ("TASK_STATE_AUTH_REQUIRED", TaskState::AuthRequired),
    ];

    for (input, expected) in cases {
        let actual = TaskState::try_from(*input)?;
        assert_eq!(actual, *expected, "input: {input}");
    }
    Ok(())
}

#[test]
fn task_state_try_from_legacy_format_fails() {
    assert!(TaskState::try_from("working").is_err());
    assert!(TaskState::try_from("inputRequired").is_err());
    assert!(TaskState::try_from("AUTH_REQUIRED").is_err());
}

#[test]
fn task_state_try_from_invalid_returns_error() {
    let result = TaskState::try_from("bogus_state");
    assert!(result.is_err());
}
