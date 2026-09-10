//! Assistant-output repetition detection concern.
//!
//! Detects when the agent emits the same normalized assistant segment
//! repeatedly with no tool calls, indicating a doom loop where the model
//! keeps producing the same text instead of taking a different action.

use std::sync::{Arc, Mutex};

use super::doom_loop::DOOM_LOOP_BACKOFF_LIMIT;

/// Number of consecutive identical normalized segments that trip detection.
pub const OUTPUT_REPETITION_THRESHOLD: usize = 5;

/// First-phase steering text returned when repetition is first detected.
pub const OUTPUT_REPETITION_MESSAGE: &str =
    "Output repetition detected; commit to a different action.";

/// Prefix for the stop text surfaced by the executor when the run is stopped.
pub const OUTPUT_REPETITION_STOP_PREFIX: &str = "Output repetition stopped:";

/// Backoff-phase steering text returned on repeated detections.
pub const OUTPUT_REPETITION_BACKOFF_MESSAGE: &str = "Output repetition persisted: the same assistant output repeated after steering. \
     Change your approach: write different content, use a tool, or ask the user for guidance.";

/// Session-scoped state tracking the last normalized segment, how many
/// consecutive times it has repeated, the escalation ladder within a turn
/// attempt, and the recent streaming delta chunks for intra-stream detection.
#[derive(Debug, Clone, Default)]
pub struct RepetitionState {
    last_segment: Option<String>,
    consecutive: usize,
    escalation_count: usize,
    recent_deltas: Vec<String>,
}

/// The escalation level of an output-repetition detection within one turn
/// attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepetitionDetection {
    /// First detection in this turn attempt: steer the model.
    First(String),
    /// Backoff detection in this turn attempt: stronger steering.
    Backoff(String),
    /// Stop detection in this turn attempt: stop the run.
    Stop(String),
}

impl RepetitionState {
    /// Clears the escalation ladder (escalation count and recent deltas) while
    /// keeping the session-scoped segment tracking.
    pub fn reset_ladder(&mut self) {
        self.escalation_count = 0;
        self.recent_deltas.clear();
    }

    /// Increments the escalation counter and returns the detection phase for
    /// this turn attempt: 1 → First, 2..=3 → Backoff, 4+ → Stop.
    pub fn escalate(&mut self) -> RepetitionDetection {
        self.escalation_count += 1;
        if self.escalation_count == 1 {
            RepetitionDetection::First(OUTPUT_REPETITION_MESSAGE.to_string())
        } else if self.escalation_count <= 1 + DOOM_LOOP_BACKOFF_LIMIT {
            RepetitionDetection::Backoff(OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string())
        } else {
            RepetitionDetection::Stop(format!(
                "{OUTPUT_REPETITION_STOP_PREFIX} the assistant kept repeating the same output \
                 after repeated steering. The run was stopped."
            ))
        }
    }
}

/// Detects assistant-output repetition across stream-response finishes.
#[derive(Clone)]
pub struct OutputRepetitionDetector;

impl OutputRepetitionDetector {
    /// Records one completed assistant response and returns the detection phase
    /// when the same normalized segment has repeated
    /// [`OUTPUT_REPETITION_THRESHOLD`] consecutive times with no tool call.
    ///
    /// A response containing a tool call, or a response whose normalized
    /// segment differs from the previous one, resets the counter and the
    /// escalation ladder. Empty text is ignored and leaves the state unchanged.
    pub fn record_response(
        state: &Arc<Mutex<RepetitionState>>,
        text: &str,
        had_tool_calls: bool,
    ) -> Option<RepetitionDetection> {
        let normalized = normalize(text);

        let mut state = state.lock().expect("output repetition mutex poisoned");

        if had_tool_calls {
            state.last_segment = None;
            state.consecutive = 0;
            state.reset_ladder();
            return None;
        }

        if normalized.is_empty() {
            return None;
        }
        if state.last_segment.as_deref() != Some(normalized.as_str()) {
            state.last_segment = Some(normalized);
            state.consecutive = 1;
            state.reset_ladder();
        } else {
            state.consecutive += 1;
        }

        if state.consecutive >= OUTPUT_REPETITION_THRESHOLD {
            Some(state.escalate())
        } else {
            None
        }
    }

    /// Records one completed model turn from its canonical assistant content
    /// and returns the detection phase when the same normalized segment has
    /// repeated [`OUTPUT_REPETITION_THRESHOLD`] consecutive times with no tool
    /// call.
    pub fn record_turn_content(
        state: &Arc<Mutex<RepetitionState>>,
        content: &[rig::message::AssistantContent],
    ) -> Option<RepetitionDetection> {
        let mut text = String::new();
        let mut tool_calls = 0usize;
        for item in content {
            match item {
                rig::message::AssistantContent::Text(t) => text.push_str(&t.text),
                rig::message::AssistantContent::ToolCall(_) => tool_calls += 1,
                _ => {}
            }
        }
        Self::record_response(state, &text, tool_calls > 0)
    }

    /// Records one streaming delta chunk and returns the detection phase when
    /// the last `OUTPUT_REPETITION_THRESHOLD` deltas are all identical. Mirrors
    /// `DoomLoopState::check_and_record` (doom_loop.rs:45-70) with String deltas
    /// instead of (String, String) signatures.
    pub fn record_delta(
        state: &Arc<Mutex<RepetitionState>>,
        delta: &str,
    ) -> Option<RepetitionDetection> {
        let normalized = normalize(delta);
        if normalized.is_empty() {
            return None;
        }
        let mut state = state.lock().expect("output repetition mutex poisoned");
        state.recent_deltas.push(normalized);
        if state.recent_deltas.len() < OUTPUT_REPETITION_THRESHOLD {
            return None;
        }
        let last_n =
            &state.recent_deltas[state.recent_deltas.len() - OUTPUT_REPETITION_THRESHOLD..];
        let first = &last_n[0];
        if !last_n.iter().all(|d| d == first) {
            return None;
        }
        Some(state.escalate())
    }
}

// region:    --- Support

/// Normalizes assistant text: trims surrounding whitespace and lowercases.
fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

// endregion: --- Support

#[cfg(test)]
#[path = "output_repetition_test.rs"]
mod output_repetition_test;
