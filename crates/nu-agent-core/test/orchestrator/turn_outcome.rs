use nu_protocol::{LabeledError, Span, Value};

use super::turn_outcome::TurnOutcome;

#[test]
fn turn_outcome_success_contains_value() {
    let value = Value::string("test", Span::test_data());
    let outcome = TurnOutcome::Success(value.clone());

    match outcome {
        TurnOutcome::Success(v) => assert_eq!(v, value),
        _ => panic!("Expected Success variant"),
    }
}

#[test]
fn turn_outcome_cancelled_is_distinct() {
    let outcome = TurnOutcome::Cancelled;

    match outcome {
        TurnOutcome::Cancelled => {}
        _ => panic!("Expected Cancelled variant"),
    }
}

#[test]
fn turn_outcome_error_contains_labeled_error() {
    let error = LabeledError::new("test error");
    let outcome = TurnOutcome::Error(error.clone());

    match outcome {
        TurnOutcome::Error(e) => assert_eq!(e.msg, "test error"),
        _ => panic!("Expected Error variant"),
    }
}

#[test]
fn turn_outcome_from_ok_result_creates_success() {
    let value = Value::nothing(Span::test_data());
    let result: Result<Value, LabeledError> = Ok(value.clone());
    let outcome = TurnOutcome::from_result(result, false);

    match outcome {
        TurnOutcome::Success(v) => assert_eq!(v, value),
        _ => panic!("Expected Success variant"),
    }
}

#[test]
fn turn_outcome_from_err_result_with_cancelled_flag_creates_cancelled() {
    let error = LabeledError::new("operation cancelled");
    let result: Result<Value, LabeledError> = Err(error);
    let outcome = TurnOutcome::from_result(result, true);

    match outcome {
        TurnOutcome::Cancelled => {}
        _ => panic!("Expected Cancelled variant"),
    }
}

#[test]
fn turn_outcome_from_err_result_without_cancelled_flag_creates_error() {
    let error = LabeledError::new("actual error");
    let result: Result<Value, LabeledError> = Err(error.clone());
    let outcome = TurnOutcome::from_result(result, false);

    match outcome {
        TurnOutcome::Error(e) => assert_eq!(e.msg, "actual error"),
        _ => panic!("Expected Error variant"),
    }
}
