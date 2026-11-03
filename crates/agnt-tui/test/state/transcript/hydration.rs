use super::*;

use agnt_render::tool_view::ToolView;

// ---------------------------------------------------------------------------
// Hydration replay rebuilds tool entries
// ---------------------------------------------------------------------------

/// Replay rebuilds one tool entry per call with its terminal status, and lands
/// a persisted display in the entry's gate-preview slot — main's hydrated shape.
#[test]
fn hydration_reconstructs_tool_entries_and_display() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec: a single finished tool call.
    hydrate_by_replay(
        &mut state,
        vec![
            UiMessageSnapshot::new("tool", "→ {\"namespace\":\"prod\"}")
                .with_tool_name("k8s__list_pods".to_string())
                .with_tool_details(
                    Some(r#"{"namespace":"prod"}"#.to_string()),
                    Some("[]".to_string()),
                    Some(true),
                ),
        ],
        None,
    );

    // -- Check: one Tool entry, not a start entry plus separate finish entries.
    assert_eq!(state.transcript.len(), 1);
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool_entry) = entry else {
        return Err("expected Tool entry".into());
    };
    assert!(tool_entry.call.call_line().contains("namespace"));
    assert_eq!(tool_entry.status, ItemStatus::Done);

    // -- Setup & Fixtures: an edit row whose own result carries the diff.
    let mut with_body = AppState::default();
    let snapshots = vec![
        UiMessageSnapshot::new("tool", r#"{"path":"a.rs","diff":"--- a\n+++ b\n"}"#)
            .with_tool_name("edit".to_string())
            .with_tool_details(
                Some(r#"{"path":"a.rs"}"#.to_string()),
                Some(r#"{"path":"a.rs","diff":"--- a\n+++ b\n"}"#.to_string()),
                Some(true),
            ),
    ];

    // -- Exec
    hydrate_by_replay(&mut with_body, snapshots, None);

    // -- Check: one entry; the body is derived from the result data, and the
    // gate-preview slot stays empty because no diff was ever in the transcript.
    assert_eq!(with_body.transcript.len(), 1, "one call, one entry");
    let entry = with_body
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool_entry) = entry else {
        return Err("expected Tool entry".into());
    };
    assert_eq!(tool_entry.status, ItemStatus::Done);
    assert!(
        tool_entry.preview.is_none(),
        "the replay carries no gate content"
    );
    let result = tool_entry.result.as_ref().ok_or("should have the body")?;
    let body: String = result
        .project_lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        body.contains("+++ b"),
        "the persisted diff is the completion body"
    );

    // -- Setup & Fixtures: one role-"user" snapshot with multi-line content.
    let mut user = AppState::default();

    // -- Exec
    hydrate_by_replay(
        &mut user,
        vec![UiMessageSnapshot::new("user", "line1\n\nline2")],
        None,
    );

    // -- Check: one User entry whose rail is contiguous on every rendered row.
    assert_eq!(
        user.transcript.len(),
        1,
        "a multi-line user message must hydrate as one entry"
    );
    let entry = user
        .transcript
        .entries()
        .first()
        .ok_or("should have user entry")?;
    let TranscriptEntry::Message(message) = entry else {
        return Err("expected User message entry".into());
    };
    assert_eq!(message.role, MessageRole::User);
    assert_eq!(message.markdown, "line1\n\nline2");

    // -- Exec: the entry renders through the message widget at the pane width.
    let width = 80usize;
    let mut backend = crate::widgets::RatatuiBackend::new(80, 8);
    let rows = agnt_render::RenderBackend::measure(&backend, entry, width);
    agnt_render::RenderBackend::render(
        &mut backend,
        entry,
        agnt_render::RowRect::new(0, width, rows),
        &agnt_render::Theme::from(&TuiTheme::default()),
    );

    // -- Check: every rendered row carries the user rail at list-local 0.
    assert!(
        rows >= 2,
        "fixture must render more than one row; got {rows}"
    );
    for y in 0..u16::try_from(rows).unwrap_or(u16::MAX) {
        assert_eq!(
            backend.buffer()[(0, y)].symbol(),
            "╎",
            "row {y} must carry the user rail at list-local 0"
        );
    }
    Ok(())
}
