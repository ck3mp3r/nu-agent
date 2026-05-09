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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThresholdCompactionPolicy {
    threshold: usize,
    tolerance: usize,
    hysteresis_margin: usize,
}

impl ThresholdCompactionPolicy {
    pub(crate) fn new(threshold: usize, tolerance: usize, hysteresis_margin: usize) -> Self {
        Self {
            threshold,
            tolerance,
            hysteresis_margin,
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

impl CompactionTriggerPolicy for ThresholdCompactionPolicy {
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

        if state.armed() {
            if usage >= self.lower_bound() {
                state.disarm();
                return CompactionTriggerDecision::Fire {
                    source: CompactionTriggerSource::AutoThreshold,
                    reason: "threshold_reached".to_string(),
                };
            }

            return CompactionTriggerDecision::NoFire {
                reason: "below_lower_bound".to_string(),
            };
        }

        if usage <= self.rearm_bound() {
            state.rearm();
            return CompactionTriggerDecision::NoFire {
                reason: "rearmed".to_string(),
            };
        }

        CompactionTriggerDecision::NoFire {
            reason: "disarmed".to_string(),
        }
    }
}
