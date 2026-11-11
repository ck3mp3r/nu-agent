use super::*;

use agnt_runtime::conversation::turn::{TurnBudget, TurnEnd, TurnFailure};
use nu_protocol::{LabeledError, Span, Value};

type TResult = core::result::Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// StopReason mapping
// ---------------------------------------------------------------------------

#[test]
fn test_map_stop_category_to_stop_reason_completed_returns_end_turn() {
    // -- Exec
    let stop_reason = map_stop_category_to_stop_reason(StopCategory::Completed);

    // -- Check
    assert_eq!(stop_reason, StopReason::EndTurn);
}

#[test]
fn test_map_stop_category_to_stop_reason_cancelled_returns_cancelled() {
    // -- Exec
    let stop_reason = map_stop_category_to_stop_reason(StopCategory::Cancelled);

    // -- Check
    assert_eq!(stop_reason, StopReason::Cancelled);
}

#[test]
fn test_map_stop_category_to_stop_reason_max_tokens_returns_max_tokens() {
    // -- Exec
    let stop_reason = map_stop_category_to_stop_reason(StopCategory::MaxTokens);

    // -- Check
    assert_eq!(stop_reason, StopReason::MaxTokens);
}

#[test]
fn test_map_stop_category_to_stop_reason_max_turns_returns_max_turn_requests() {
    // -- Exec
    let stop_reason = map_stop_category_to_stop_reason(StopCategory::MaxTurns);

    // -- Check
    assert_eq!(stop_reason, StopReason::MaxTurnRequests);
}

// ---------------------------------------------------------------------------
// Turn end → StopCategory
// ---------------------------------------------------------------------------

/// WHEN a turn completes, THE stop category SHALL be `Completed`.
#[test]
fn test_stop_category_completed_end_reports_completed() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Completed {
        value: Value::nothing(Span::unknown()),
    };

    // -- Exec
    let category = stop_category("session-1", &end);

    // -- Check
    assert_eq!(category, StopCategory::Completed);
    Ok(())
}

/// WHEN the turn end is a stop, THE stop category SHALL be `Cancelled`.
#[test]
fn test_stop_category_cancelled_end_reports_cancelled() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Cancelled {
        value: Value::nothing(Span::unknown()),
    };

    // -- Exec
    let category = stop_category("session-1", &end);

    // -- Check
    assert_eq!(category, StopCategory::Cancelled);
    Ok(())
}

/// WHEN a failed turn ended on the tool-turn limit, THE stop category SHALL be
/// `MaxTurns` — from the typed budget, not from the message text.
#[test]
fn test_stop_category_failed_end_on_max_turns_reports_max_turns() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Failed(TurnFailure::with_budget(
        LabeledError::new("the turn ran out of tool calls"),
        TurnBudget::MaxTurns { limit: 3 },
    ));

    // -- Exec
    let category = stop_category("session-1", &end);

    // -- Check
    assert_eq!(category, StopCategory::MaxTurns);
    Ok(())
}

/// WHEN a failed turn ended on the output-token ceiling, THE stop category SHALL
/// be `MaxTokens`.
#[test]
fn test_stop_category_failed_end_on_max_tokens_reports_max_tokens() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Failed(TurnFailure::with_budget(
        LabeledError::new("the reply was cut short"),
        TurnBudget::MaxTokens,
    ));

    // -- Exec
    let category = stop_category("session-1", &end);

    // -- Check
    assert_eq!(category, StopCategory::MaxTokens);
    Ok(())
}

/// WHEN a failed turn ended on no budget, THE stop category SHALL be
/// `Completed`, and a message that names a limit SHALL NOT change it.
#[test]
fn test_stop_category_failed_end_without_a_budget_reports_completed() -> TResult {
    // -- Setup & Fixtures
    let end = TurnEnd::Failed(TurnFailure::from_error(LabeledError::new(
        "Turn failed: max turns (3) exceeded",
    )));

    // -- Exec
    let category = stop_category("session-1", &end);

    // -- Check
    assert_eq!(
        category,
        StopCategory::Completed,
        "only the typed budget names a budget stop"
    );
    Ok(())
}
