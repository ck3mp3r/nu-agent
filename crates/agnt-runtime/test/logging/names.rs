//! The instance log file name: one owner for the format, and the scan that
//! recognises exactly the names it composes.

use super::{instance_log_file_name, is_instance_log_file_name};

// region:    --- Tests

/// The format has one owner: every name the store composes is a name its
/// collection scan accepts, and a legacy or foreign name is not.
#[test]
fn test_logging_instance_log_file_name_round_trips_through_the_scan() {
    // -- Setup & Fixtures
    let composed = [
        instance_log_file_name(Some("alpha"), 42),
        instance_log_file_name(Some("a-b-c"), 7),
        instance_log_file_name(None, 9),
    ];

    // -- Exec & Check
    for name in composed {
        assert!(
            is_instance_log_file_name(&name),
            "a composed instance log name must pass the scan: {name}"
        );
    }
    for foreign in ["tui-42.log", "a2a-42.log", "agent-42.log.bak", "notes.txt"] {
        assert!(
            !is_instance_log_file_name(foreign),
            "a name the store does not compose must not pass the scan: {foreign}"
        );
    }
}

// endregion: --- Tests
