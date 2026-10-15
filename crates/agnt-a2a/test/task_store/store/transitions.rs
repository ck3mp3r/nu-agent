use super::*;

#[test]
fn test_is_valid_transition_all_valid() {
    // Test every valid transition
    assert!(super::is_valid_transition(
        &TaskState::Submitted,
        &TaskState::Working
    ));
    assert!(super::is_valid_transition(
        &TaskState::Submitted,
        &TaskState::Canceled
    ));
    assert!(super::is_valid_transition(
        &TaskState::Submitted,
        &TaskState::Rejected
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::InputRequired
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Completed
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Failed
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Canceled
    ));
    assert!(super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Rejected
    ));
    assert!(super::is_valid_transition(
        &TaskState::InputRequired,
        &TaskState::Working
    ));
    assert!(super::is_valid_transition(
        &TaskState::InputRequired,
        &TaskState::Canceled
    ));
}

#[test]
fn test_is_valid_transition_all_invalid() {
    // Terminal states reject everything
    for terminal in &[
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ] {
        for target in &[
            TaskState::Submitted,
            TaskState::Working,
            TaskState::InputRequired,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Canceled,
            TaskState::Rejected,
        ] {
            assert!(
                !super::is_valid_transition(terminal, target),
                "{:?} -> {:?} should be invalid",
                terminal,
                target
            );
        }
    }
    // Explicit invalid: Working -> Submitted
    assert!(!super::is_valid_transition(
        &TaskState::Working,
        &TaskState::Submitted
    ));
}
