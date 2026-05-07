use nu_protocol::LabeledError;

const LLM_CALL_CANCELLED_MESSAGE: &str = "LLM call cancelled";

pub fn llm_call_cancelled_error() -> LabeledError {
    LabeledError::new(LLM_CALL_CANCELLED_MESSAGE)
}

pub fn is_llm_call_cancelled(error: &LabeledError) -> bool {
    error.msg == LLM_CALL_CANCELLED_MESSAGE
}
