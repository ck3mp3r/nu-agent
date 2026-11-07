//! Shared fixtures for the output repetition test modules.
//!
//! Holds the imports, the `Result` alias, and the detector-driving helpers
//! every topical output-repetition test file needs. Sub-files pull them in
//! with `use super::*;`.

pub(super) use std::sync::{Arc, Mutex};

pub(super) use crate::hook::output_repetition::{
    OUTPUT_REPETITION_THRESHOLD, OutputRepetitionDetector, RepetitionDetection, RepetitionState,
};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Drives the detector through `OUTPUT_REPETITION_THRESHOLD` identical
/// text-only responses, returning every detection in order.
pub(super) fn drive_to_threshold(
    state: &Arc<Mutex<RepetitionState>>,
) -> Vec<Option<RepetitionDetection>> {
    let mut detections = Vec::new();
    for _ in 0..OUTPUT_REPETITION_THRESHOLD {
        detections.push(OutputRepetitionDetector::record_response(
            state,
            "I will do the thing.",
            false,
        ));
    }
    detections
}

/// Repeats `unit` `count` times joined with `sep`, plus the separator before
/// the first unit (matching how live streams emit `sep` between units).
pub(super) fn repeat_separated(unit: &str, sep: &str, count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        if i > 0 {
            text.push_str(sep);
        }
        text.push_str(unit);
    }
    text
}

/// Drives the ladder to `escalation_count == target` through cross-turn
/// `finish_turn` escalations. Identical full-text turns accumulate
/// `consecutive`; every `threshold`th identical turn completion escalates
/// the ladder one step — so `target` escalations require
/// `threshold * target` identical turn completions.
pub(super) fn drive_escalation_to(
    state: &Arc<Mutex<RepetitionState>>,
    target: usize,
) -> Result<()> {
    let content = [rig::message::AssistantContent::text("I will do the thing.")];
    // Identical full-text turns accumulate `consecutive`; the ladder
    // escalates once each time `consecutive` reaches a multiple of the
    // threshold. `target` escalations therefore need `threshold * target`
    // identical turn completions.
    let total = OUTPUT_REPETITION_THRESHOLD * target;
    for _ in 0..total {
        let _detection = OutputRepetitionDetector::finish_turn(state, content.as_slice());
    }
    let count = state.lock().map_err(|_| "should lock")?.escalation_count;
    assert!(
        count >= target,
        "fixture must reach at least the target escalation (got {count})"
    );
    Ok(())
}
