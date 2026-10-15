use super::*;

// ---------------------------------------------------------------------------
// StopReason mapping
// ---------------------------------------------------------------------------

#[test]
fn test_map_turn_outcome_completed_returns_end_turn() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::Completed);

    // -- Check
    assert_eq!(stop_reason, StopReason::EndTurn);
}

#[test]
fn test_map_turn_outcome_cancelled_returns_cancelled() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::Cancelled);

    // -- Check
    assert_eq!(stop_reason, StopReason::Cancelled);
}

#[test]
fn test_map_turn_outcome_max_tokens_returns_max_tokens() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::MaxTokens);

    // -- Check
    assert_eq!(stop_reason, StopReason::MaxTokens);
}

#[test]
fn test_map_turn_outcome_max_turns_returns_max_turn_requests() {
    // -- Exec
    let stop_reason = map_turn_outcome_to_stop_reason(TurnOutcome::MaxTurns);

    // -- Check
    assert_eq!(stop_reason, StopReason::MaxTurnRequests);
}
