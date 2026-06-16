use crate::compaction::CompactionStrategy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionTriggerSource {
    AutoThreshold,
    SlashCompact,
}

impl CompactionTriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoThreshold => "auto_threshold",
            Self::SlashCompact => "slash_compact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionTriggerDecision {
    Fire {
        source: CompactionTriggerSource,
        reason: String,
        strategy: CompactionStrategy,
    },
    NoFire {
        reason: String,
    },
}

pub trait CompactionTriggerPolicy {
    fn evaluate(&self, total_tokens: Option<u64>) -> CompactionTriggerDecision;
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenCompactionPolicy {
    context_window_max: u64,
    threshold_pct: f64,
    strategy: CompactionStrategy,
}

impl TokenCompactionPolicy {
    pub fn new(context_window_max: u64, threshold_pct: f64, strategy: CompactionStrategy) -> Self {
        Self {
            context_window_max,
            threshold_pct,
            strategy,
        }
    }
}

impl CompactionTriggerPolicy for TokenCompactionPolicy {
    fn evaluate(&self, total_tokens: Option<u64>) -> CompactionTriggerDecision {
        let Some(tokens) = total_tokens else {
            return CompactionTriggerDecision::NoFire {
                reason: "no_token_data".to_string(),
            };
        };

        if self.context_window_max == 0 {
            return CompactionTriggerDecision::NoFire {
                reason: "zero_context_window".to_string(),
            };
        }

        let usage_pct = tokens as f64 / self.context_window_max as f64;
        if usage_pct >= self.threshold_pct {
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: format!(
                    "token_usage_{:.0}pct_of_{:.0}pct_threshold",
                    usage_pct * 100.0,
                    self.threshold_pct * 100.0
                ),
                strategy: self.strategy,
            }
        } else {
            CompactionTriggerDecision::NoFire {
                reason: format!(
                    "below_threshold_{:.0}pct_of_{:.0}pct",
                    usage_pct * 100.0,
                    self.threshold_pct * 100.0
                ),
            }
        }
    }
}
