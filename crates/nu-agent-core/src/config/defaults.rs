//! Default values for ModelRoleConfig fields when not specified by the user.
//! These are the built-in fallbacks used at usage sites via unwrap_or().

/// Default maximum context tokens. Drives compaction threshold.
pub const MAX_CONTEXT_TOKENS: u32 = 128_000;

/// Default read timeout for HTTP streaming responses in seconds.
pub const READ_TIMEOUT_SECS: u64 = 120;

/// Default maximum tool calls allowed per sub-turn.
pub const MAX_TOOL_CALLS_PER_SUBTURN: usize = 25;

/// Default maximum bytes of a single tool result before truncation.
pub const MAX_TOOL_RESULT_BYTES: usize = 20_000;

/// Default maximum retry attempts for transient errors.
pub const MAX_RETRIES: u8 = 3;

/// Default maximum provider-feedback retries per user turn for
/// model-correctable errors. Each retry appends one model-facing feedback
/// message to the session memory and re-runs the turn once.
pub const MAX_PROVIDER_FEEDBACK_RETRIES: u8 = 2;

/// Default base backoff in ms, doubles each attempt, capped at 30_000ms.
pub const RETRY_BASE_DELAY_MS: u64 = 1000;

/// Default context warning threshold (fraction of model_context_tokens).
pub const CONTEXT_WARNING_THRESHOLD: f32 = 0.6;

/// Default proactive compaction threshold (fraction of the context window at
/// which the hook triggers compaction).
pub const PROACTIVE_THRESHOLD_PCT: f64 = 0.8;

/// Default MCP HTTP read timeout in seconds.
pub const MCP_READ_TIMEOUT_SECS: u64 = 120;
