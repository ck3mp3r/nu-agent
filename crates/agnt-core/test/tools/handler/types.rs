//! `ToolErrorKind` → rig `ToolExecutionError` mapping tests.

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Every handler error kind maps to the matching rig error kind and the
/// message passes through unchanged.
#[test]
fn test_tools_handler_types_execution_error_for_kind_maps_every_kind() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(ToolErrorKind, rig::tool::ToolErrorKind)> = vec![
        (
            ToolErrorKind::Validation,
            rig::tool::ToolErrorKind::InvalidArgs,
        ),
        (ToolErrorKind::Timeout, rig::tool::ToolErrorKind::Timeout),
        (
            ToolErrorKind::Authorization,
            rig::tool::ToolErrorKind::PermissionDenied,
        ),
        (ToolErrorKind::Transport, rig::tool::ToolErrorKind::Network),
        (ToolErrorKind::Runtime, rig::tool::ToolErrorKind::Other),
        (ToolErrorKind::Unknown, rig::tool::ToolErrorKind::Other),
        // A not-run call maps to rig's cancelled kind: the transcript's not-run
        // class (`NOT_RUN_ERROR_KINDS`) reads this kind name, so the live row
        // finishes ⊘ exactly as the replayed row does.
        (ToolErrorKind::NotRun, rig::tool::ToolErrorKind::Cancelled),
    ];

    // -- Exec & Check
    for (kind, expected) in cases {
        let mapped = execution_error_for_kind(kind.clone(), "boom".to_string());
        assert_eq!(
            mapped.kind(),
            expected,
            "kind {} must map to the matching rig kind",
            kind.as_str()
        );
        assert_eq!(
            mapped.message(),
            "boom",
            "kind {} must pass the message through unchanged",
            kind.as_str()
        );
    }
    Ok(())
}

/// The mapping keeps the message byte-identical, including punctuation and
/// non-ASCII text.
#[test]
fn test_tools_handler_types_execution_error_for_kind_keeps_message_verbatim() -> Result<()> {
    // -- Setup & Fixtures
    let message = "Tool 'grep' timed out after 30s — ünïcode".to_string();

    // -- Exec
    let mapped = execution_error_for_kind(ToolErrorKind::Timeout, message.clone());

    // -- Check
    assert_eq!(mapped.kind(), rig::tool::ToolErrorKind::Timeout);
    assert_eq!(mapped.message(), message);
    Ok(())
}
