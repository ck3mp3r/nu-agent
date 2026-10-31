use super::*;
use agnt_core::transcript::entry::TranscriptEntry;

/// Coordinator hydration skips blank lines, shows a role outside the persisted
/// vocabulary as a system notice, reconstructs tool rows matching the live
/// shape, and uses the tool's tailored call line.
#[test]
fn coordinator_hydration_and_tool_rows() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "line1\n\nline2"),
            UiMessageSnapshot::new("assistant", "\n\nreply\n"),
            UiMessageSnapshot::new("tool", "tool output"),
            UiMessageSnapshot::new("mystery", "system fallback"),
        ],
        None,
    );

    // -- Check: blank lines are preserved; a line outside the closed role set
    // replays as a system notice.
    let lines = coordinator.state().transcript.entries().to_vec();
    assert_eq!(
        lines
            .iter()
            .map(|entry| (entry_to_role(entry), entry_text(entry)))
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::User, "line1\n\nline2".to_string()),
            (TranscriptRole::Assistant, "reply".to_string()),
            (TranscriptRole::System, "system fallback".to_string()),
        ]
    );

    // -- Setup & Fixtures: a finished tool call.
    let mut tool = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    tool.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "→ {\"namespace\":\"prod\"}")
                .with_tool_name("k8s__list_pods".to_string())
                .with_tool_details(
                    Some("{\"namespace\":\"prod\"}".to_string()),
                    Some("[{\"name\":\"api-0\"}]".to_string()),
                    Some(true),
                ),
        ],
        None,
    );

    // -- Check: one Tool entry matching the live shape.
    assert_eq!(tool.state().transcript.len(), 1);
    let tool_entries = tool.state().transcript.entries();
    let tool_entry = tool_entries.first().ok_or("should have tool entry")?;
    assert!(matches!(tool_entry, TranscriptEntry::Tool(_)));
    let TranscriptEntry::Tool(tool) = tool_entry else {
        return Err("expected Tool variant".into());
    };
    assert!(tool.call.call_line.summary.contains("namespace"));
    assert_eq!(tool_status_of(tool_entry), Some(ItemStatus::Done));

    // -- Setup & Fixtures: a nu tool call.
    let mut nu = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    nu.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "→ \"ls\"")
                .with_tool_name("nu".to_string())
                .with_tool_details(
                    Some(r#"{"command":"ls | select name type size"}"#.to_string()),
                    None,
                    Some(true),
                ),
        ],
        None,
    );

    // -- Check: the nu tool's tailored call line.
    let nu_entries = nu.state().transcript.entries();
    let entry = nu_entries.first().ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool variant".into());
    };
    assert_eq!(tool.call.call_line.summary, "⏱ 120s");

    // -- Setup & Fixtures: an edit tool call.
    let mut edit = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    edit.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "→ \"a.rs\" (diff)")
                .with_tool_name("edit".to_string())
                .with_tool_details(
                    Some(
                        r#"{"path":"a.rs","operation":{"type":"create","content":"x"}}"#
                            .to_string(),
                    ),
                    None,
                    Some(true),
                ),
        ],
        None,
    );

    // -- Check: the edit tool's tailored inline call line.
    let edit_entries = edit.state().transcript.entries();
    let entry = edit_entries.first().ok_or("should have tool entry")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool variant".into());
    };
    assert_eq!(tool.call.call_line.summary, "→ a.rs (diff)");

    Ok(())
}

/// Coordinator hydration projects user and assistant markdown, keeps
/// unsupported markdown readable, handles malformed markdown without dropping
/// the message, and reprojects on resize.
#[tokio::test]
async fn coordinator_hydration_markdown_projection() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "# user stays literal"),
            UiMessageSnapshot::new("assistant", "# heading\n\n`x`"),
        ],
        None,
    );

    // -- Check: user text is literal; assistant markdown projects.
    let raw_lines: Vec<(TranscriptRole, String)> = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .map(|entry| (entry_to_role(entry), entry_text(entry)))
        .collect();
    assert!(
        raw_lines
            .iter()
            .any(|(r, t)| *r == TranscriptRole::User && t.contains("user stays literal"))
    );
    let assistant_projected: Vec<String> = raw_lines
        .iter()
        .filter(|(r, _)| *r == TranscriptRole::Assistant)
        .flat_map(|(_, md)| crate::markdown::render_markdown_lines(md, None))
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert!(assistant_projected.iter().any(|l| l.contains("heading")));
    assert!(assistant_projected.iter().any(|l| l.contains("x")));

    // -- Setup & Fixtures: unsupported markdown.
    let mut unsupported = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("unsupported_fallback.md");

    // -- Exec
    unsupported.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", &markdown)],
        None,
    );

    // -- Check: tables render with separators; alt text is readable.
    let projected_lines: Vec<String> = unsupported
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|b| {
            matches!(
                b,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            )
        })
        .flat_map(|b| crate::markdown::render_markdown_lines(&entry_text(b), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("col") && line.contains("val"))
    );
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("a") && line.contains("b"))
    );
    assert!(projected_lines.iter().any(|line| line.contains("│")));
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("alt (image: https://img.example/x.png)"))
    );

    // -- Setup & Fixtures: malformed markdown.
    let mut malformed = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("malformed.md");

    // -- Exec
    malformed.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", &markdown)],
        None,
    );

    // -- Check: the message is not dropped.
    let malformed_blocks = malformed.state().transcript.entries();
    let assistant_entries = malformed_blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            )
        })
        .collect::<Vec<_>>();
    assert!(!assistant_entries.is_empty());
    let projected_text: String = assistant_entries
        .iter()
        .flat_map(|b| crate::markdown::render_markdown_lines(&entry_text(b), None))
        .flat_map(|l| l.spans.into_iter())
        .map(|s| s.text)
        .collect();
    assert!(projected_text.contains("fn main() {"));

    // -- Setup & Fixtures: a memoized projection across repeated messages.
    let mut memoized = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("fenced_code_blocks.md");

    // -- Exec
    memoized.enqueue_event(SystemEvent::InferenceChunk {
        delta: markdown.clone(),
        aggregated: markdown.clone(),
    });
    memoized.drain_transport();
    memoized.enqueue_event(SystemEvent::InferenceChunk {
        delta: markdown.clone(),
        aggregated: markdown,
    });
    memoized.drain_transport();

    // -- Check: the second message replaces the first.
    assert_eq!(
        memoized
            .state()
            .transcript
            .entries()
            .iter()
            .filter(|b| matches!(
                b,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            ))
            .count(),
        1
    );

    // -- Setup & Fixtures: resize reprojection.
    let mut driver = RenderLoopDriver::new(120, 30);
    let markdown = markdown_fixture("fenced_code_blocks.md");
    driver
        .coordinator_mut()
        .enqueue_event(SystemEvent::InferenceChunk {
            delta: markdown.clone(),
            aggregated: markdown,
        });
    driver.coordinator_mut().drain_transport();

    // -- Exec
    for (columns, rows) in [(100, 28), (140, 42), (80, 24)] {
        driver
            .advance(&[DriveEvent::Key(TerminalEvent::Resize(
                crate::interaction::input::TerminalResize { columns, rows },
            ))])
            .await?;
    }

    // -- Check: the resize path completes without error.
    Ok(())
}

/// An assistant message event sanitizes pseudo tags and control tags at
/// projection time.
#[test]
fn assistant_message_event_sanitizes_pseudo_and_control_tags() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let text = "prefix\n[code:json]\n{\"ok\":true}\n[/code]\n<system-reminder>hidden</system-reminder>\nsuffix";

    // -- Exec
    coordinator.enqueue_event(SystemEvent::InferenceChunk {
        delta: text.to_string(),
        aggregated: text.to_string(),
    });
    coordinator.drain_transport();

    // -- Check
    let projected_lines: Vec<String> = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|b| {
            matches!(
                b,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            )
        })
        .flat_map(|b| crate::markdown::render_markdown_lines(&entry_text(b), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();
    assert!(projected_lines.iter().any(|l| l.contains("prefix")));
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("{\"ok\":true}"))
    );
    assert!(projected_lines.iter().any(|l| l.contains("suffix")));
    assert!(!projected_lines.iter().any(|line| line.contains("[code:")));
    assert!(!projected_lines.iter().any(|line| line.contains("[/code]")));
    assert!(
        !projected_lines
            .iter()
            .any(|line| line.contains("<system-reminder>"))
    );
    assert!(!projected_lines.iter().any(|line| line.contains("hidden")));
    Ok(())
}

/// A tool display snapshot that directly follows its owning tool call lands in
/// that call's gate-preview slot — no companion display entry is pushed.
#[test]
fn hydration_pairs_tool_display_with_owning_call() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let display = ToolDisplay {
        title: None,
        sections: vec![agnt_core::protocol::event::ToolDisplaySection {
            label: None,
            kind: agnt_core::transcript::ir::ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls".to_string(),
            stats: None,
        }],
    };

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "→ \"ls\"")
                .with_tool_name("nu".to_string())
                .with_tool_details(Some(r#"{"command":"ls"}"#.to_string()), None, Some(true)),
            UiMessageSnapshot::new("tool_display", String::new()).with_tool_display(display),
            UiMessageSnapshot::new("tool", "→ \"a.txt\"")
                .with_tool_name("read_file".to_string())
                .with_tool_details(Some(r#"{"path":"a.txt"}"#.to_string()), None, Some(true)),
        ],
        None,
    );

    // -- Check: two tool entries; the display lands in the FIRST call's
    // gate-preview slot, and the second call keeps no display.
    let entries = coordinator.state().transcript.entries();
    assert_eq!(entries.len(), 2, "the display pushes no companion entry");
    let TranscriptEntry::Tool(first) = &entries[0] else {
        return Err("expected Tool entry".into());
    };
    let TranscriptEntry::Tool(second) = &entries[1] else {
        return Err("expected Tool entry".into());
    };
    assert!(
        first.preview.is_some(),
        "the display must attach to its owning call's preview slot"
    );
    assert!(
        first.result.is_none(),
        "a hydrated completion carries no display of its own"
    );
    assert!(
        second.preview.is_none(),
        "the next call must not receive the display"
    );
    Ok(())
}

/// Hydration does not duplicate lines on a single call, and an empty message
/// snapshot leaves the empty-session behavior unchanged.
#[test]
fn coordinator_hydration_regression() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "dup-check"),
            UiMessageSnapshot::new("assistant", "dup-check-reply"),
        ],
        None,
    );

    // -- Check: no duplicate lines.
    let user_count = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|entry| entry_text(entry) == "dup-check")
        .count();
    let assistant_count = coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|entry| entry_text(entry) == "dup-check-reply")
        .count();
    assert_eq!(user_count, 1);
    assert_eq!(assistant_count, 1);

    // -- Setup & Fixtures: an empty snapshot.
    let mut empty = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    empty.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    // -- Check: the empty-session behavior is unchanged.
    let state = empty.state();
    assert!(state.transcript.entries().is_empty());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    Ok(())
}
