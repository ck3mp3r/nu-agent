use nu_protocol::{LabeledError, Value};

/// Outcome of executing a conversation turn.
///
/// This replaces `Result<Value, LabeledError>` to provide structured
/// handling of cancellation vs errors without string matching.
///
/// # Current Implementation
///
/// Cancellation is detected by string matching error messages:
/// - v1 path: "LLM call cancelled"
/// - v2 path: "Turn cancelled: ..."
///
/// # Future Improvement
///
/// The v1 and v2 paths should be updated to return `TurnOutcome` directly,
/// eliminating the need for string matching. The `TurnError.cancelled` flag
/// provides the typed information needed but is currently lost when converting
/// to `LabeledError`.
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    /// Turn completed successfully with a result value
    Success(Value),
    /// Turn was cancelled by the user
    Cancelled,
    /// Turn failed with an error
    Error(LabeledError),
}

impl TurnOutcome {
    /// Convert a `Result<Value, LabeledError>` to a `TurnOutcome`.
    ///
    /// The `cancelled` flag indicates whether the error represents a cancellation.
    /// This is used to convert from the v1/v2 return types which still use `Result`.
    pub fn from_result(result: Result<Value, LabeledError>, cancelled: bool) -> Self {
        match result {
            Ok(value) => TurnOutcome::Success(value),
            Err(error) => {
                if cancelled {
                    TurnOutcome::Cancelled
                } else {
                    TurnOutcome::Error(error)
                }
            }
        }
    }
}
