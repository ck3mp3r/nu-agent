//! The lag telemetry: a hook whose reader stalls past the ring's capacity says
//! so.
//!
//! The wording half stays a unit test. The facade half — the line reaching the
//! running instance's own log file — installs the process logger, so it lives in
//! `lag_telemetry_gated.rs` behind the `integration` gate.

/// WHEN a hook's reader lags the record stream, THEN the re-sync SHALL log a
/// warn-level line naming the hook and the dropped count.
#[test]
fn test_hooks_runner_lag_warning_names_the_hook_and_the_dropped_count() {
    // -- Exec
    let warning = crate::hooks::runner::lag_warning("audit", 76);

    // -- Check
    assert!(
        warning.contains("audit"),
        "the warning must name the hook: {warning}"
    );
    assert!(
        warning.contains("76"),
        "the warning must name the dropped count: {warning}"
    );
}
