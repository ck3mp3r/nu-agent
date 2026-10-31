use super::*;

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
    assert!(tool_entry.call.call_line.summary.contains("namespace"));
    assert_eq!(tool_entry.status, ItemStatus::Done);

    // -- Setup & Fixtures: a tool call plus a standalone persisted display.
    let mut with_display = AppState::default();
    let snapshots = vec![
        UiMessageSnapshot::new("tool", "→ {\"path\":\"a.rs\"}")
            .with_tool_name("edit".to_string())
            .with_tool_details(Some(r#"{"path":"a.rs"}"#.to_string()), None, Some(true)),
        UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(ToolDisplay {
            title: Some("edit a.rs".to_string()),
            sections: vec![ToolDisplaySection {
                label: Some("changes".to_string()),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    ];

    // -- Exec
    hydrate_by_replay(&mut with_display, snapshots, None);

    // -- Check: the display lands in the tool entry's gate-preview slot.
    assert_eq!(
        with_display.transcript.len(),
        1,
        "the display must not add a companion entry"
    );
    let entry = with_display
        .transcript
        .entries()
        .first()
        .ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool_entry) = entry else {
        return Err("expected Tool entry".into());
    };
    assert_eq!(tool_entry.status, ItemStatus::Done);
    assert!(
        tool_entry.preview.is_some(),
        "the persisted display docks in the preview slot"
    );
    assert!(
        tool_entry.result.is_none(),
        "a hydrated completion carries no display of its own"
    );
    let preview = tool_entry.preview.as_ref().ok_or("should have preview")?;
    let result_text: String = preview
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
    assert!(result_text.contains("+++ b"));

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
