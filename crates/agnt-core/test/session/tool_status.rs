//! The structural tool status: the store round trip and the disposition map.
//!
//! The status is written as bytes and read back as the same variant. No test
//! here reads a result's text: the classification input is the structured
//! disposition, and the persisted line carries the tag the enum writes.

use crate::session::tool_status::{
    TOOL_STATUS_FILE, ToolStatus, ToolStatusRecord, read_statuses, status_of_result, write_statuses,
};
use crate::types::{CallId, ToolExecutionError, ToolExecutionResult, ToolOutput};
use rig_memory::{Files, MemoryStore};

use serde_json::json;

pub(crate) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// WHEN every status is written, THE store SHALL carry one line per call and
/// each line SHALL deserialize back to the exact variant — the status is
/// structural on both sides.
#[test]
fn test_tool_status_store_round_trip_keeps_every_variant() -> Result<()> {
    // -- Setup & Fixtures
    let store = MemoryStore::default();
    let cases = [
        ("call_success", ToolStatus::Success),
        ("call_failed", ToolStatus::Failed),
        ("call_cancelled", ToolStatus::Cancelled),
    ];
    let records: Vec<ToolStatusRecord> = cases
        .iter()
        .map(|(call, status)| ToolStatusRecord {
            call: CallId::from_wire(*call),
            status: *status,
        })
        .collect();

    // -- Exec
    write_statuses(&store, &records)?;
    let bytes = store.read_file(TOOL_STATUS_FILE)?;
    let text = std::str::from_utf8(&bytes)?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let read = read_statuses(&store);

    // -- Check: one line per record, the tag the enum writes, and the variant
    // back — the raw line carries bytes, the code carries variants.
    assert_eq!(lines.len(), cases.len(), "one line per call: {text}");
    for ((call, status), line) in cases.iter().zip(lines) {
        let parsed: serde_json::Value = serde_json::from_str(line)?;
        assert_eq!(
            parsed,
            json!({
                "call": {"provider": call},
                "status": serde_json::to_value(status)?,
            }),
            "{call}: the line carries the record structurally"
        );
        let record: ToolStatusRecord = serde_json::from_str(line)?;
        assert_eq!(record.call, CallId::from_wire(*call));
        assert_eq!(record.status, *status, "{call}: the exact variant");
    }
    for (call, status) in cases {
        assert_eq!(
            read.get(&CallId::from_wire(call)).copied(),
            Some(status),
            "{call}: the reader answers the written variant"
        );
    }
    Ok(())
}

/// WHEN a call settles, THE status SHALL come from the structured disposition:
/// a success, a policy skip (the permission denial), a cancelled error kind,
/// and everything else a failure.
#[test]
fn test_tool_status_status_of_result_maps_every_disposition() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, ToolExecutionResult, ToolStatus)> = vec![
        (
            "success",
            ToolExecutionResult::success(ToolOutput::text("ok")),
            ToolStatus::Success,
        ),
        (
            "cancelled kind",
            ToolExecutionResult::failed(ToolExecutionError::cancelled("stopped")),
            ToolStatus::Cancelled,
        ),
        (
            "runtime failure",
            ToolExecutionResult::failed(ToolExecutionError::other("broke")),
            ToolStatus::Failed,
        ),
        (
            "intentional refusal",
            ToolExecutionResult::failed(ToolExecutionError::refused("no")),
            ToolStatus::Failed,
        ),
        (
            "policy skip (the permission denial)",
            ToolExecutionResult::skipped("denied by policy"),
            ToolStatus::Cancelled,
        ),
    ];

    // -- Exec & Check
    for (label, result, expected) in cases {
        assert_eq!(status_of_result(&result), expected, "{label}");
    }
    Ok(())
}

/// WHEN the session carries no status file, THE reader SHALL answer nothing — a
/// session saved before the statuses existed reads its rows by their stored
/// verdict, unchanged.
#[test]
fn test_tool_status_read_of_a_missing_file_is_empty() -> Result<()> {
    // -- Setup & Fixtures
    let store = MemoryStore::default();

    // -- Exec
    let statuses = read_statuses(&store);

    // -- Check
    assert!(statuses.is_empty(), "no file, no statuses: {statuses:?}");
    Ok(())
}

/// WHEN the status file carries a line nothing reads, THE reader SHALL skip that
/// line and keep every record around it.
#[test]
fn test_tool_status_read_skips_an_unreadable_line() -> Result<()> {
    // -- Setup & Fixtures
    let store = MemoryStore::default();
    let kept = ToolStatusRecord {
        call: CallId::from_wire("call_kept"),
        status: ToolStatus::Cancelled,
    };
    let lines = format!("not json\n{}\n\n", serde_json::to_string(&kept)?);
    store.write_file(TOOL_STATUS_FILE, lines.as_bytes())?;

    // -- Exec
    let statuses = read_statuses(&store);

    // -- Check
    assert_eq!(
        statuses.len(),
        1,
        "the readable line survives: {statuses:?}"
    );
    assert_eq!(statuses.get(&kept.call).copied(), Some(kept.status));
    Ok(())
}
