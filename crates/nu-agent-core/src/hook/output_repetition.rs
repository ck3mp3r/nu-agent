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
/// Names the failure and gives a concrete alternative action.
pub const OUTPUT_REPETITION_MESSAGE: &str = "Output repetition detected: your responses repeated the same content. \
     Change your approach: write different content, call a tool, or ask the user for guidance.";

/// Prefix for the stop text surfaced by the executor when the run is stopped.
pub const OUTPUT_REPETITION_STOP_PREFIX: &str = "Output repetition stopped:";

/// Character-growth interval between intra-stream suffix checks. The
/// aggregated text must grow by this many bytes before `check_aggregated`
/// runs again, bounding the check cost across a long stream.
pub const OUTPUT_REPETITION_CHECK_INTERVAL: usize = 100;

/// Size of the normalized suffix window (in bytes) that `check_aggregated`
/// inspects. Caps the per-check cost at O(WINDOW²/threshold) regardless of
/// total aggregated length.
pub const OUTPUT_REPETITION_WINDOW: usize = 2000;

/// Backoff-phase steering text returned on repeated detections. Names the
/// failure and gives a concrete alternative action.
pub const OUTPUT_REPETITION_BACKOFF_MESSAGE: &str = "Output repetition persisted: your responses repeated the same content again after steering. \
     Change your approach now: write different content, call a tool, or ask the user for guidance.";

/// Minimum byte length of a repeated suffix unit that counts as a
/// meaningful repetition. Shorter units (single bytes, dashes, ellipses)
/// occur in ordinary text and must not trip the detector.
pub const MIN_UNIT: usize = 8;

/// Session-scoped state tracking the last normalized segment, how many
/// consecutive times it has repeated, the escalation ladder within a turn
/// attempt, the aggregated-length mark for the intra-stream growth
/// throttle, and the intra-stream detection flag consumed at the turn
/// boundary by [`OutputRepetitionDetector::finish_turn`].
#[derive(Debug, Clone, Default)]
pub struct RepetitionState {
    last_segment: Option<String>,
    consecutive: usize,
    escalation_count: usize,
    last_check_len: usize,
    intra_stream_detected: bool,
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

/// The intra-stream action for a detection: the ladder level resolved at
/// detection time by [`OutputRepetitionDetector::check_aggregated`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepetitionAction {
    /// Ladder below Stop level (escalation count 1..=3): warn and continue —
    /// the turn completes and steering happens at the turn boundary via
    /// `retry_with_feedback`.
    Warn,
    /// Ladder at Stop level (escalation count >= 4): kill the stream
    /// mid-turn; the executor surfaces the stop reason.
    Stop,
}

impl RepetitionState {
    /// Clears the escalation ladder (escalation count and growth-throttle
    /// mark) while keeping the session-scoped segment tracking.
    pub fn reset_ladder(&mut self) {
        self.escalation_count = 0;
        self.last_check_len = 0;
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

    /// Turn-boundary entry point for the `on_model_turn_finished` hook:
    /// consumes the intra-stream detection flag left by
    /// [`Self::check_aggregated`] and escalates the ladder when the flag was
    /// set, or falls back to the cross-turn full-text comparison.
    ///
    /// Flag path: intra-stream detection already PROVED the repetition within
    /// the turn, so the ladder escalates directly — routing through
    /// [`Self::record_response`] would compare the full normalized text for
    /// the first time and miscount it as `consecutive = 1`, leaving the
    /// ladder unmoved on the live failure this path exists to catch. The
    /// growth-throttle mark is reset so the next turn's intra-stream checks
    /// are not skipped by stale growth bookkeeping. The flag is always
    /// consumed (read + cleared) on every call, and the escalation happens
    /// within the same lock scope as the flag consume.
    ///
    /// No-flag path: delegates to [`Self::record_turn_content`] (the mutex is
    /// not reentrant, so the guard is dropped first).
    pub fn finish_turn(
        state: &Arc<Mutex<RepetitionState>>,
        content: &[rig::message::AssistantContent],
    ) -> Option<RepetitionDetection> {
        let detected = {
            let mut state = state.lock().expect("output repetition mutex poisoned");
            let detected = state.intra_stream_detected;
            state.intra_stream_detected = false;
            if detected {
                // The repetition was already proven intra-stream: escalate
                // directly, bypassing the cross-turn full-text comparison.
                state.last_check_len = 0;
                Some(state.escalate())
            } else {
                None
            }
        };
        if detected.is_some() {
            return detected;
        }
        Self::record_turn_content(state, content)
    }

    /// Detect-only check: whether the aggregated stream text ends with a
    /// phrase-length unit repeated [`OUTPUT_REPETITION_THRESHOLD`] (5) times.
    /// Runs on the full text streamed so far this turn (`event.aggregated`)
    /// rather than individual deltas — streaming servers tokenize differently
    /// each pass, so delta chunks are never reliably identical, but the
    /// repetition is fully visible in the aggregated text as a suffix pattern.
    ///
    /// - `Some(Stop)`: every detection kills the stream mid-turn — the stream
    ///   stops on the FIRST detection, not after ladder escalations. The
    ///   executor's stop-to-steering retry (append steering + reset ladder +
    ///   re-run, capped by `MAX_REPETITION_STOP_RETRIES`) handles escalation:
    ///   stop → steering → retry; a second stop is terminal. The
    ///   intra-stream flag is NOT set: rig delivers the stop before the
    ///   turn-boundary hook fires, so a set flag would leak into the NEXT
    ///   turn's `finish_turn` and falsely escalate an innocent turn. The
    ///   ladder is left untouched here — it is reset by the executor's
    ///   stop-to-steering retry.
    /// - `None` (no detection): continue without touching any state.
    ///
    /// Gated by character-growth throttling: runs only when aggregated has
    /// grown by [`OUTPUT_REPETITION_CHECK_INTERVAL`] (100) chars since the
    /// last check, bounding the O(WINDOW²/threshold) suffix check to
    /// sub-millisecond cost (note 5c39995b §3-4).
    pub fn check_aggregated(
        state: &Arc<Mutex<RepetitionState>>,
        aggregated: &str,
    ) -> Option<RepetitionAction> {
        let mut state = state.lock().expect("output repetition mutex poisoned");
        // Growth throttle: skip until the aggregated text has grown enough to
        // plausibly contain a new full repetition window. The mark is not
        // updated when the gate stays closed.
        if aggregated.len() < state.last_check_len + OUTPUT_REPETITION_CHECK_INTERVAL {
            return None;
        }
        state.last_check_len = aggregated.len();

        // Normalize with whitespace collapse, then restrict the suffix check to
        // the sliding window. `floor_char_boundary` keeps slicing panic-free on
        // multibyte input.
        let normalized = normalize(aggregated);
        let window_start = normalized.len().saturating_sub(OUTPUT_REPETITION_WINDOW);
        let window_start = normalized.floor_char_boundary(window_start);
        let window = &normalized[window_start..];
        let window_len = window.len();
        if window_len < OUTPUT_REPETITION_THRESHOLD {
            return None;
        }

        // Suffix repetition check: try each candidate unit length from
        // [`MIN_UNIT`] up to window_len/threshold; the window must end with
        // that unit repeated `OUTPUT_REPETITION_THRESHOLD` times. Byte-slice
        // comparison: byte slicing never panics, and the suffix length is
        // exactly `unit_len * threshold` so the chunks align with the unit
        // bytes for any genuine repetition. The `MIN_UNIT` floor keeps
        // short-byte sequences (single chars, dashes) from false-tripping.
        let window_bytes = window.as_bytes();
        for unit_len in MIN_UNIT..=window_len / OUTPUT_REPETITION_THRESHOLD {
            let unit = &window_bytes[window_len - unit_len..];
            let required = unit_len * OUTPUT_REPETITION_THRESHOLD;
            let suffix = &window_bytes[window_len - required..];
            if suffix.chunks_exact(unit_len).all(|chunk| chunk == unit) {
                // Every detection stops the stream mid-turn: the executor's
                // stop-to-steering retry handles escalation (steering + ladder
                // reset + re-run, terminal on the second stop). The flag is
                // deliberately NOT set: rig kills the stream before the
                // turn-boundary hook fires, so the flag would leak into the
                // next turn's `finish_turn`.
                return Some(RepetitionAction::Stop);
            }
        }
        None
    }
}

// region:    --- Support

/// Normalizes assistant text: trims surrounding whitespace, lowercases, and
/// collapses all whitespace runs (spaces, tabs, newlines) to single spaces.
/// The whitespace collapse makes the aggregated suffix check robust to
/// whitespace variation between repetitions (note 5c39995b §3).
fn normalize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// endregion: --- Support

#[cfg(test)]
#[path = "output_repetition_test.rs"]
mod output_repetition_test;
