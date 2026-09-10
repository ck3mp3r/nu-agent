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
/// consecutive times it has repeated, and the escalation ladder within a turn
/// attempt.
#[derive(Debug, Clone, Default)]
pub struct RepetitionState {
    last_segment: Option<String>,
    consecutive: usize,
    escalation_count: usize,
    pending_steering: Option<String>,
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
    /// Clears the escalation ladder (escalation count and pending steering)
    /// while keeping the session-scoped segment tracking.
    pub fn reset_ladder(&mut self) {
        self.escalation_count = 0;
        self.pending_steering = None;
    }

    /// Increments the escalation counter and returns the detection phase for
    /// this turn attempt: 1 → First, 2..=3 → Backoff, 4+ → Stop.
    ///
    /// For the First and Backoff phases, the steering text is stored in
    /// [`Self::pending_steering`] so the next `on_completion_call` can inject it
    /// into the model's context. The Stop phase does not set it.
    pub fn escalate(&mut self) -> RepetitionDetection {
        self.escalation_count += 1;
        if self.escalation_count == 1 {
            let message = OUTPUT_REPETITION_MESSAGE.to_string();
            self.pending_steering = Some(message.clone());
            RepetitionDetection::First(message)
        } else if self.escalation_count <= 1 + DOOM_LOOP_BACKOFF_LIMIT {
            let message = OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string();
            self.pending_steering = Some(message.clone());
            RepetitionDetection::Backoff(message)
        } else {
            RepetitionDetection::Stop(format!(
                "{OUTPUT_REPETITION_STOP_PREFIX} the assistant kept repeating the same output \
                 after repeated steering. The run was stopped."
            ))
        }
    }

    /// Takes the pending steering text, clearing it (one-shot). Returns `None`
    /// when no steering is pending.
    pub fn take_pending_steering(&mut self) -> Option<String> {
        self.pending_steering.take()
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
        let segment = first_segment(&normalized);

        let mut state = state.lock().expect("output repetition mutex poisoned");

        if had_tool_calls {
            state.last_segment = None;
            state.consecutive = 0;
            state.reset_ladder();
            return None;
        }

        let segment = segment?;
        if state.last_segment.as_deref() != Some(segment.as_str()) {
            state.last_segment = Some(segment);
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

    /// Detects repetition within a single streaming response.
    ///
    /// Stateless: a pure function of the full assistant text streamed so far
    /// this turn. Splits the raw text on `.` and `\n`, normalizes each piece,
    /// and trips when the run of consecutive identical segments ENDING at the
    /// last complete segment reaches [`OUTPUT_REPETITION_THRESHOLD`].
    /// Alternating segments never trip. The escalation ladder is applied by
    /// the caller via [`RepetitionState::escalate`].
    pub fn check_streaming(aggregated: &str) -> bool {
        let ends_with_boundary = aggregated.ends_with(['.', '\n']);
        let pieces: Vec<String> = aggregated
            .split(['.', '\n'])
            .map(normalize)
            .filter(|s| !s.is_empty())
            .collect();

        let mut current_run = 0usize;
        let mut prev: Option<&str> = None;
        let mut last_complete_run = 0usize;

        for (i, piece) in pieces.iter().enumerate() {
            if prev == Some(piece.as_str()) {
                current_run += 1;
            } else {
                current_run = 1;
                prev = Some(piece.as_str());
            }
            // A piece is a complete segment when it is followed by a boundary:
            // either it is not the final piece, or the text ends with a boundary.
            let is_last = i == pieces.len() - 1;
            if !is_last || ends_with_boundary {
                last_complete_run = current_run;
            }
        }

        last_complete_run >= OUTPUT_REPETITION_THRESHOLD
    }
}

// region:    --- Support

/// Normalizes assistant text: trims surrounding whitespace, collapses runs of
/// whitespace to single spaces, and lowercases.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Splits normalized text on `.` and `\n` and returns the first non-empty
/// segment, or `None` when the text is empty.
fn first_segment(normalized: &str) -> Option<String> {
    normalized
        .split(['.', '\n'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

// endregion: --- Support

#[cfg(test)]
#[path = "output_repetition_test.rs"]
mod output_repetition_test;
