use super::*;

/// Coordinator hydration skips blank lines, maps unknown roles to system,
/// reconstructs tool rows matching the live shape, uses the tool's tailored
/// call line, and parses persisted tool status lines.
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

    // -- Check: blank lines are preserved; unknown roles map to system.
    let lines = coordinator.state().transcript.blocks().to_vec();
    assert_eq!(
        lines
            .iter()
            .map(|block| (block_to_role(block), block.source.plain_text()))
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
            UiMessageSnapshot::new("tool", "tool[k8s__list_pods] → {} · done").with_tool_details(
                Some("{\"namespace\":\"prod\"}".to_string()),
                Some("[{\"name\":\"api-0\"}]".to_string()),
                Some(true),
            ),
        ],
        None,
    );

    // -- Check: one Tool block matching the live shape.
    assert_eq!(tool.state().transcript.len(), 1);
    let tool_block = &tool.state().transcript.blocks()[0];
    assert!(matches!(tool_block.source, BlockSource::Tool { .. }));
    let BlockSource::Tool { call, .. } = &tool_block.source else {
        return Err("expected Tool variant".into());
    };
    assert!(call.summary.contains("namespace"));
    assert_eq!(tool_block.status, Some(ItemStatus::Done));

    // -- Setup & Fixtures: a nu tool call.
    let mut nu = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    nu.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[nu] → \"ls\" · done").with_tool_details(
                Some(r#"{"command":"ls | select name type size"}"#.to_string()),
                None,
                Some(true),
            ),
        ],
        None,
    );

    // -- Check: the nu tool's tailored call line.
    let block = &nu.state().transcript.blocks()[0];
    let BlockSource::Tool { call, .. } = &block.source else {
        return Err("expected Tool variant".into());
    };
    assert_eq!(call.summary, "⏱ 120s");

    // -- Setup & Fixtures: an edit tool call.
    let mut edit = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    edit.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[edit] → \"a.rs\" · done").with_tool_details(
                Some(r#"{"path":"a.rs","operation":{"type":"create","content":"x"}}"#.to_string()),
                None,
                Some(true),
            ),
        ],
        None,
    );

    // -- Check: the edit tool's tailored inline call line.
    let block = &edit.state().transcript.blocks()[0];
    let BlockSource::Tool { call, .. } = &block.source else {
        return Err("expected Tool variant".into());
    };
    assert_eq!(call.summary, "→ a.rs (diff)");

    // -- Exec & Check: the persisted tool status line parser.
    assert_eq!(
        crate::state::parse_persisted_tool_status_line(
            "tool[k8s__list_pods] → {\"namespace\":\"prod\"} · done"
        ),
        Some(("k8s__list_pods", "{\"namespace\":\"prod\"}", true))
    );
    assert_eq!(
        crate::state::parse_persisted_tool_status_line("tool[gh__run] → {} · failed"),
        Some(("gh__run", "{}", false))
    );
    assert_eq!(
        crate::state::parse_persisted_tool_status_line("tool[gh__run] → {}"),
        None
    );
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
        .blocks()
        .iter()
        .map(|block| (block_to_role(block), block.source.plain_text()))
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
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .flat_map(|b| crate::markdown::render_markdown_lines(&b.source.plain_text(), None))
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
    let assistant_entries = malformed
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    assert!(!assistant_entries.is_empty());
    let projected_text: String = assistant_entries
        .iter()
        .flat_map(|b| crate::markdown::render_markdown_lines(&b.source.plain_text(), None))
        .flat_map(|l| l.spans.into_iter())
        .map(|s| s.text)
        .collect();
    assert!(projected_text.contains("fn main() {"));

    // -- Setup & Fixtures: a memoized projection across repeated messages.
    let mut memoized = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("fenced_code_blocks.md");

    // -- Exec
    memoized.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: markdown.clone(),
        text: markdown.clone(),
    });
    memoized.drain_transport();
    memoized.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: markdown.clone(),
        text: markdown,
    });
    memoized.drain_transport();

    // -- Check: the second message replaces the first.
    assert_eq!(
        memoized
            .state()
            .transcript
            .blocks()
            .iter()
            .filter(|b| matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            ))
            .count(),
        1
    );

    // -- Setup & Fixtures: resize reprojection.
    let mut driver = RenderLoopDriver::new(120, 30);
    let markdown = markdown_fixture("fenced_code_blocks.md");
    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::AssistantMessage {
            delta: markdown.clone(),
            text: markdown,
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
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: text.to_string(),
        text: text.to_string(),
    });
    coordinator.drain_transport();

    // -- Check
    let projected_lines: Vec<String> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .flat_map(|b| crate::markdown::render_markdown_lines(&b.source.plain_text(), None))
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
        .blocks()
        .iter()
        .filter(|b| b.source.plain_text() == "dup-check")
        .count();
    let assistant_count = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| b.source.plain_text() == "dup-check-reply")
        .count();
    assert_eq!(user_count, 1);
    assert_eq!(assistant_count, 1);

    // -- Setup & Fixtures: an empty snapshot.
    let mut empty = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    empty.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    // -- Check: the empty-session behavior is unchanged.
    let state = empty.state();
    assert!(state.transcript.blocks().is_empty());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    Ok(())
}
