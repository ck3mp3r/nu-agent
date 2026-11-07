use super::*;

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
