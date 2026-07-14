use super::*;

// ---------------------------------------------------------------------------
// TaskState
// ---------------------------------------------------------------------------

#[test]
fn task_state_all_variants_serde() {
    use serde_test::{Token, assert_tokens};

    let cases: &[(TaskState, &str)] = &[
        (TaskState::Unspecified, "UNSPECIFIED"),
        (TaskState::Submitted, "SUBMITTED"),
        (TaskState::Working, "WORKING"),
        (TaskState::InputRequired, "INPUT_REQUIRED"),
        (TaskState::Completed, "COMPLETED"),
        (TaskState::Failed, "FAILED"),
        (TaskState::Canceled, "CANCELED"),
        (TaskState::Rejected, "REJECTED"),
        (TaskState::AuthRequired, "AUTH_REQUIRED"),
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
    let result: Result<TaskState, _> = serde_json::from_str("\"unknown_state\"");
    assert!(result.is_err());
}

#[test]
fn task_state_display_output() {
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
}

#[test]
fn task_state_try_from_valid() {
    assert_eq!(
        TaskState::try_from("submitted").unwrap(),
        TaskState::Submitted
    );
    assert_eq!(TaskState::try_from("working").unwrap(), TaskState::Working);
    assert_eq!(
        TaskState::try_from("inputRequired").unwrap(),
        TaskState::InputRequired
    );
    assert_eq!(
        TaskState::try_from("completed").unwrap(),
        TaskState::Completed
    );
    assert_eq!(TaskState::try_from("failed").unwrap(), TaskState::Failed);
    assert_eq!(
        TaskState::try_from("canceled").unwrap(),
        TaskState::Canceled
    );
    assert_eq!(
        TaskState::try_from("rejected").unwrap(),
        TaskState::Rejected
    );
}

#[test]
fn task_state_try_from_invalid_returns_error() {
    let result = TaskState::try_from("bogus_state");
    assert!(result.is_err());
}
