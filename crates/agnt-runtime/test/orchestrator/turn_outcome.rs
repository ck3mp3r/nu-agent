use nu_protocol::{LabeledError, Span, Value};

use crate::conversation::turn::{TurnBudget, TurnEnd, TurnExhaustion, TurnFailure};

use super::turn_outcome::{TurnOutcome, TurnResult};

type TResult = core::result::Result<(), Box<dyn std::error::Error>>;

/// WHEN the executor ends a turn completed, THE result SHALL carry its response
/// value.
#[test]
fn test_turn_result_from_completed_end_carries_the_response() -> TResult {
    // -- Setup & Fixtures
    let value = Value::string("test", Span::test_data());

    // -- Exec
    let result = TurnResult::from_end(TurnEnd::Completed {
        value: value.clone(),
    });

    // -- Check
    match result.outcome {
        TurnOutcome::Success(returned) => assert_eq!(returned, value),
        other => return Err(format!("expected a Success outcome, got {other:?}").into()),
    }
    Ok(())
}

/// WHEN the executor ends a turn cancelled, THE result SHALL report `Cancelled`
/// from the variant alone.
#[test]
fn test_turn_result_from_cancelled_end_reports_cancelled() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Cancelled {
        value: Value::nothing(Span::test_data()),
    };

    // -- Exec
    let result = TurnResult::from_end(end);

    // -- Check
    assert!(
        matches!(result.outcome, TurnOutcome::Cancelled),
        "a cancelled turn end must map to Cancelled; got {:?}",
        result.outcome
    );
    Ok(())
}

/// WHEN a failed turn's message reads like a cancellation, THE result SHALL
/// still report `Error`: the variant decides, never the text.
#[test]
fn test_turn_result_failed_end_ignores_cancellation_looking_text() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Failed(TurnFailure::from_error(LabeledError::new(
        "Turn cancelled: not really",
    )));

    // -- Exec
    let result = TurnResult::from_end(end);

    // -- Check
    match result.outcome {
        TurnOutcome::Error(error) => assert_eq!(error.msg, "Turn cancelled: not really"),
        other => {
            return Err(format!("a failed turn must map to Error, got {other:?}").into());
        }
    }
    Ok(())
}

/// WHEN a turn failed, THE result SHALL carry the failure's user-facing error.
#[test]
fn test_turn_result_failed_end_carries_the_user_facing_error() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Failed(TurnFailure::with_budget(
        LabeledError::new("Turn failed: output budget exhausted"),
        TurnBudget::MaxTokens,
    ));

    // -- Exec
    let result = TurnResult::from_end(end);

    // -- Check
    match result.outcome {
        TurnOutcome::Error(error) => {
            assert_eq!(error.msg, "Turn failed: output budget exhausted")
        }
        other => return Err(format!("expected an Error outcome, got {other:?}").into()),
    }
    Ok(())
}

/// WHEN a turn ran for an external task, THE result SHALL carry that task's id,
/// so the consumer pairs the completion with the turn that produced it.
#[test]
fn test_turn_result_with_task_id_binds_the_turn_attribution() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Completed {
        value: Value::nothing(Span::test_data()),
    };

    // -- Exec
    let result = TurnResult::from_end(end).with_task_id(Some("task-1".to_string()));

    // -- Check
    assert_eq!(result.task_id.as_deref(), Some("task-1"));
    assert!(
        matches!(result.outcome, TurnOutcome::Success(_)),
        "the attribution must not change the outcome"
    );
    Ok(())
}

/// WHEN a user's prompt ran, THE result SHALL carry no task id.
#[test]
fn test_turn_result_without_a_task_id_stays_unattributed() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Completed {
        value: Value::nothing(Span::test_data()),
    };

    // -- Exec
    let result = TurnResult::from_end(end).with_task_id(None);

    // -- Check
    assert_eq!(result.task_id, None);
    Ok(())
}

/// The turn end's exhaustions stay distinct data: a retry-exhausted failure
/// names the attempts, a steering-exhausted one names no output.
#[test]
fn test_turn_failure_keeps_the_exhaustion_distinct() -> TResult {
    // -- Setup & Fixtures
    let retries = TurnFailure {
        error: LabeledError::new("Turn failed after 2 retries. boom"),
        budget: None,
        exhaustion: Some(TurnExhaustion::Retries { attempts: 2 }),
    };
    let no_output = TurnFailure {
        error: LabeledError::new("Turn failed: conversation too long. No output was produced."),
        budget: None,
        exhaustion: Some(TurnExhaustion::NoOutput),
    };

    // -- Exec & Check
    assert_eq!(
        retries.exhaustion,
        Some(TurnExhaustion::Retries { attempts: 2 })
    );
    assert_eq!(no_output.exhaustion, Some(TurnExhaustion::NoOutput));
    assert_ne!(retries.exhaustion, no_output.exhaustion);
    Ok(())
}
