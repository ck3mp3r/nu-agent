use serde::{Deserialize, Serialize};

/// Strategy for compacting messages when threshold is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Summarize old messages and keep a recent verbatim window.
    #[serde(
        rename = "sliding_summary",
        alias = "truncate",
        alias = "sliding",
        alias = "summarize"
    )]
    SlidingSummary,
    /// Drop old messages, keep only the last N. No summarization.
    #[serde(rename = "sliding_window")]
    SlidingWindow,
    /// Keep newest messages that fit within a token budget. No summarization.
    #[serde(rename = "token_truncate")]
    TokenTruncate,
}

impl CompactionStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlidingSummary => "sliding_summary",
            Self::SlidingWindow => "sliding_window",
            Self::TokenTruncate => "token_truncate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionOutcome {
    pub summarized_count: usize,
    pub kept_recent_count: usize,
    pub summary_text: String,
    /// Token count captured from the summarization streaming response.
    /// `None` for non-SlidingSummary strategies (no LLM call) or when the
    /// provider did not yield a `Final` usage chunk.
    pub summary_total_tokens: Option<u64>,
}

/// Configuration for compaction behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionParams {
    /// Strategy to use for compaction.
    pub compaction_strategy: CompactionStrategy,
    /// Number of recent messages to keep during truncation compaction.
    pub keep_recent: usize,
    /// Maximum token budget for TokenTruncate strategy.
    pub token_budget: Option<usize>,
}

impl Default for CompactionParams {
    fn default() -> Self {
        Self {
            compaction_strategy: CompactionStrategy::SlidingSummary, // Canonical strategy
            keep_recent: 10,    // Default keep last 10 messages
            token_budget: None, // No token budget by default
        }
    }
}
