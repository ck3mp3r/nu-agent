//! Shared fixtures for the doom loop test modules.
//!
//! Holds the imports, the `TestResult` alias, and the action-inspection
//! helpers every topical doom-loop test file needs. Sub-files pull them in
//! with `use super::*;`.

pub(super) use crate::bus::{Bus, create_bus};
pub(super) use crate::hook::doom_loop::{DOOM_LOOP_THRESHOLD, DoomLoopDetector};
pub(super) use rig::agent::DispatchAction;
pub(super) use rig::error::ErrorKind;

pub(super) type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

/// Drives the detector through `DOOM_LOOP_THRESHOLD` identical calls, returning
/// every action in order. Call `i < DOOM_LOOP_THRESHOLD - 1` is under threshold;
/// the final call is the first detection.
pub(super) async fn drive_to_first_detection(
    detector: &DoomLoopDetector,
    bus: &Bus,
) -> Vec<Option<DispatchAction>> {
    let mut actions = Vec::new();
    for _ in 0..DOOM_LOOP_THRESHOLD {
        let action = detector
            .check_and_record("read_file", "{\"path\": \"same\"}", bus)
            .await;
        actions.push(action);
    }
    actions
}

/// Whether `action` is a skip denial: a `DispatchAction::Deny` whose report is
/// not a cancellation (a cancellation is a stop).
pub(super) fn is_skip(action: &Option<DispatchAction>) -> bool {
    matches!(action, Some(DispatchAction::Deny(report)) if report.kind != ErrorKind::Cancelled)
}

/// The reason of a skip denial, if `action` is one.
pub(super) fn skip_reason(action: &Option<DispatchAction>) -> Option<&str> {
    match action {
        Some(DispatchAction::Deny(report)) if report.kind != ErrorKind::Cancelled => {
            Some(report.message.as_str())
        }
        _ => None,
    }
}

/// The reason of a stop denial, if `action` is one.
pub(super) fn stop_reason(action: &Option<DispatchAction>) -> Option<&str> {
    match action {
        Some(DispatchAction::Deny(report)) if report.kind == ErrorKind::Cancelled => {
            Some(report.message.as_str())
        }
        _ => None,
    }
}
