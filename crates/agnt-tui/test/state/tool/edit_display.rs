use super::*;

// ---------------------------------------------------------------------------
// The derived edit body: no invented title or label, stats from the payload
// ---------------------------------------------------------------------------

/// The edit view draws the body its result payload names. It invents no title
/// and no section label — the producer supplies neither — and the stats row is
/// the payload's own counters.
#[test]
fn edit_display_derives_its_body_from_the_result_payload() -> Result<()> {
    // -- Setup & Fixtures
    let diff = "--- a/sample.txt\n+++ b/sample.txt\n@@ -1 +1 @@\n-old\n+new\n";

    // -- Exec: a payload carrying only the path and the diff.
    let mut state = AppState::default();
    reduce_ui(&mut state, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut state,
        "edit",
        r#"{"path":"sample.txt"}"#,
        &edit_payload("sample.txt", diff),
    );

    // -- Check: the diff renders, and no title or label row is invented.
    let lines: Vec<String> = state
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(lines.iter().any(|line| line.contains("+++ b/sample.txt")));
    assert!(!lines.iter().any(|line| line.contains("edit sample.txt")));
    assert!(!lines.iter().any(|line| line == "sample.txt (diff)"));

    // -- Exec: a payload that also reports its counters.
    let mut stats = AppState::default();
    reduce_ui(&mut stats, started("edit", r#"{"path":"sample.txt"}"#));
    complete_with_result(
        &mut stats,
        "edit",
        r#"{"path":"sample.txt"}"#,
        r#"{"path":"sample.txt","diff":"+new","stats":{"files_changed":1,"insertions":3,"deletions":1}}"#,
    );
    let lines: Vec<String> = stats
        .transcript
        .entries()
        .iter()
        .flat_map(extract_all_text_from_entry)
        .collect();
    assert!(lines.iter().any(|line| line.contains("files=1")));
    Ok(())
}
