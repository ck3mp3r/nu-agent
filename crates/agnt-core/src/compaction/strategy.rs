use serde::{Deserialize, Serialize};

/// Strategy for compacting messages when threshold is exceeded.
///
/// Compaction always summarizes the full context (previous summary + all
/// messages since the last marker) into a single structured summary. There is
/// no sliding window, no kept-message window, and no token truncation — the
/// summary IS the context. The enum is retained as a single-variant marker so
/// the config surface (`strategy` key, `--compaction-strategy` flag) keeps
/// parsing legacy values without a separate string type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Summarize all messages into a single summary message. No verbatim
    /// messages are kept.
    #[serde(
        rename = "sliding_summary",
        alias = "truncate",
        alias = "sliding",
        alias = "summarize",
        alias = "sliding_window",
        alias = "token_truncate"
    )]
    SlidingSummary,
}

impl CompactionStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlidingSummary => "sliding_summary",
        }
    }
}

/// Configuration for compaction behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionParams {
    /// Strategy to use for compaction.
    pub compaction_strategy: CompactionStrategy,
}

impl Default for CompactionParams {
    fn default() -> Self {
        Self {
            compaction_strategy: CompactionStrategy::SlidingSummary,
        }
    }
}
