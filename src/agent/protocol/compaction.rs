use crate::session::CompactionStrategy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionTriggerSource {
    AutoThreshold,
    SlashCompact,
}

impl CompactionTriggerSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AutoThreshold => "auto_threshold",
            Self::SlashCompact => "slash_compact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompactionTriggerDecision {
    Fire {
        source: CompactionTriggerSource,
        reason: String,
        strategy: CompactionStrategy,
    },
    FallbackFire {
        source: CompactionTriggerSource,
        reason: String,
        strategies: Vec<CompactionStrategy>,
    },
    NoFire {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactionTriggerState {
    armed: bool,
}

impl Default for CompactionTriggerState {
    fn default() -> Self {
        Self { armed: true }
    }
}

impl CompactionTriggerState {
    pub(crate) fn armed(&self) -> bool {
        self.armed
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn rearm(&mut self) {
        self.armed = true;
    }
}

pub(crate) trait CompactionTriggerPolicy {
    fn evaluate(
        &self,
        usage_signal: Option<usize>,
        state: &mut CompactionTriggerState,
    ) -> CompactionTriggerDecision;
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TwoTierCompactionPolicy {
    threshold: usize,
    tolerance: usize,
    hysteresis_margin: usize,
    proactive_threshold_pct: f32,
    primary_strategy: CompactionStrategy,
    fallback_strategies: Vec<CompactionStrategy>,
}

impl TwoTierCompactionPolicy {
    #[cfg(test)]
    pub(crate) fn new(threshold: usize, tolerance: usize, hysteresis_margin: usize) -> Self {
        Self {
            threshold,
            tolerance,
            hysteresis_margin,
            proactive_threshold_pct: 0.80,
            primary_strategy: CompactionStrategy::SlidingSummary,
            fallback_strategies: vec![CompactionStrategy::SlidingWindow],
        }
    }

    /// Create a policy with explicit strategy, proactive threshold, and fallback configuration.
    ///
    /// This is the config-aware constructor used when `SessionConfig` / `CompactionConfig`
    /// values have been resolved from plugin config + CLI flags.
    pub(crate) fn with_config(
        threshold: usize,
        tolerance: usize,
        hysteresis_margin: usize,
        primary_strategy: CompactionStrategy,
        proactive_threshold_pct: f32,
        fallback_strategies: Vec<CompactionStrategy>,
    ) -> Self {
        Self {
            threshold,
            tolerance,
            hysteresis_margin,
            proactive_threshold_pct,
            primary_strategy,
            fallback_strategies,
        }
    }

    fn lower_bound(&self) -> usize {
        self.threshold.saturating_sub(self.tolerance)
    }

    fn rearm_bound(&self) -> usize {
        self.threshold
            .saturating_sub(self.tolerance.saturating_add(self.hysteresis_margin))
    }
}

impl CompactionTriggerPolicy for TwoTierCompactionPolicy {
    fn evaluate(
        &self,
        usage_signal: Option<usize>,
        state: &mut CompactionTriggerState,
    ) -> CompactionTriggerDecision {
        let Some(usage) = usage_signal else {
            return CompactionTriggerDecision::NoFire {
                reason: "signal_unavailable".to_string(),
            };
        };

        let decision = if state.armed() {
            if usage >= self.lower_bound() {
                state.disarm();
                CompactionTriggerDecision::Fire {
                    source: CompactionTriggerSource::AutoThreshold,
                    reason: "threshold_reached".to_string(),
                    strategy: self.primary_strategy,
                }
            } else {
                CompactionTriggerDecision::NoFire {
                    reason: "below_lower_bound".to_string(),
                }
            }
        } else if usage <= self.rearm_bound() {
            state.rearm();
            if usage >= self.lower_bound() {
                state.disarm();
                CompactionTriggerDecision::Fire {
                    source: CompactionTriggerSource::AutoThreshold,
                    reason: "rearmed_and_threshold_reached".to_string(),
                    strategy: self.primary_strategy,
                }
            } else {
                CompactionTriggerDecision::NoFire {
                    reason: "rearmed".to_string(),
                }
            }
        } else if usage >= self.lower_bound() {
            state.rearm();
            state.disarm();
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "sustained_high_usage".to_string(),
                strategy: self.primary_strategy,
            }
        } else {
            CompactionTriggerDecision::NoFire {
                reason: "disarmed_below_threshold".to_string(),
            }
        };

        // Tier 2: if NoFire but approaching threshold, emit FallbackFire
        if matches!(decision, CompactionTriggerDecision::NoFire { .. }) {
            let fallback_bound = (self.threshold as f32 * 0.95) as usize;
            if usage >= fallback_bound {
                return CompactionTriggerDecision::FallbackFire {
                    source: CompactionTriggerSource::AutoThreshold,
                    reason: "approaching_threshold".to_string(),
                    strategies: self.fallback_strategies.clone(),
                };
            }
        }

        decision
    }
}
