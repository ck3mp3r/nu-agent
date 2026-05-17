use nu_protocol::LabeledError;

const LLM_CALL_CANCELLED_MESSAGE: &str = "LLM call cancelled";

/// Create a cancellation error.
///
/// Used by v1 runtime path. Can be removed once TurnOutcome is used throughout.
pub fn llm_call_cancelled_error() -> LabeledError {
    LabeledError::new(LLM_CALL_CANCELLED_MESSAGE)
}

/// Check if an error is a cancellation error using string matching.
///
/// # Deprecated Pattern
///
/// This function uses brittle string matching. The orchestrator now uses
/// `TurnOutcome` which provides structured cancellation detection. This
/// function is still used by the runtime v1 path and can be removed once
/// runtime methods return `TurnOutcome` directly.
pub fn is_llm_call_cancelled(error: &LabeledError) -> bool {
    error.msg == LLM_CALL_CANCELLED_MESSAGE
}
