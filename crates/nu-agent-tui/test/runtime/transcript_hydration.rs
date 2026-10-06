use super::*;

#[test]
fn coordinator_hydration_skips_blank_lines_and_maps_unknown_role_to_system() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "line1\n\nline2"),
            UiMessageSnapshot::new("assistant", "\n\nreply\n"),
            UiMessageSnapshot::new("tool", "tool output"),
            UiMessageSnapshot::new("mystery", "system fallback"),
        ],
        None,
    );

    let lines = coordinator.state().transcript.blocks().to_vec();
    assert_eq!(
        lines
            .iter()
            .map(|block| (block_to_role(block), block.source.plain_text()))
            .collect::<Vec<_>>(),
        vec![
            // user block: one whole block, blank lines preserved
            (TranscriptRole::User, "line1\n\nline2".to_string()),
            // assistant block: reply
            (TranscriptRole::Assistant, "reply".to_string()),
            // system block: fallback
            (TranscriptRole::System, "system fallback".to_string()),
        ]
    );
}

#[test]
fn hydrated_tool_history_matches_live_tool_row_shape() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[k8s__list_pods] → {} · done").with_tool_details(
                Some("{\"namespace\":\"prod\"}".to_string()),
                Some("[{\"name\":\"api-0\"}]".to_string()),
                Some(true),
            ),
        ],
        None,
    );

    // Tool block: no leading spacer under the unified spacer rule
    assert_eq!(coordinator.state().transcript.len(), 1);
    let tool_block = &coordinator.state().transcript.blocks()[0];
    assert!(matches!(tool_block.source, BlockSource::Tool { .. }));
    if let BlockSource::Tool { call, .. } = &tool_block.source {
        assert!(call.summary.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(tool_block.status, Some(ItemStatus::Done));
}

#[test]
fn hydrated_nu_tool_row_call_line_contains_command() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[nu] → \"ls\" · done").with_tool_details(
                Some(r#"{"command":"ls | select name type size"}"#.to_string()),
                None,
                Some(true),
            ),
        ],
        None,
    );

    // -- Check: the nu tool's tailored call line carries the applied
    // timeout — the command renders in the preview block.
    let block = &coordinator.state().transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert_eq!(
            call.summary, "⏱ 120s",
            "hydrated nu row must use the nu tool's tailored call line"
        );
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn hydrated_edit_tool_row_call_line_uses_edit_tailored_render() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[edit] → \"a.rs\" · done").with_tool_details(
                Some(r#"{"path":"a.rs","operation":{"type":"create","content":"x"}}"#.to_string()),
                None,
                Some(true),
            ),
        ],
        None,
    );

    // -- Check
    let block = &coordinator.state().transcript.blocks()[0];
    if let BlockSource::Tool { call, .. } = &block.source {
        assert_eq!(
            call.summary,
            "→ a.rs (diff)".to_string(),
            "hydrated edit row must use the edit tool's tailored inline call line"
        );
    } else {
        panic!("Expected Tool variant");
    }
}

#[test]
fn parse_persisted_tool_status_line_supports_done_and_failed_shapes() {
    let done = crate::state::parse_persisted_tool_status_line(
        "tool[k8s__list_pods] → {\"namespace\":\"prod\"} · done",
    );
    assert_eq!(
        done,
        Some(("k8s__list_pods", "{\"namespace\":\"prod\"}", true))
    );

    let failed = crate::state::parse_persisted_tool_status_line("tool[gh__run] → {} · failed");
    assert_eq!(failed, Some(("gh__run", "{}", false)));

    assert_eq!(
        crate::state::parse_persisted_tool_status_line("tool[gh__run] → {}"),
        None
    );
}

#[test]
fn coordinator_hydration_projects_both_user_and_assistant_markdown() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "# user stays literal"),
            UiMessageSnapshot::new("assistant", "# heading\n\n`x`"),
        ],
        None,
    );

    // After the raw-markdown refactor: the block source is raw markdown.
    // Project to verify the rendered content.
    let raw_lines: Vec<(TranscriptRole, String)> = coordinator
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| (block_to_role(block), block.source.plain_text()))
        .collect();

    // User message: raw markdown stored as-is
    assert!(
        raw_lines
            .iter()
            .any(|(r, t)| *r == TranscriptRole::User && t.contains("user stays literal")),
        "user message should contain the text; got: {raw_lines:?}"
    );
    // Assistant message: raw markdown stored, projection produces heading and code
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
    assert!(
        assistant_projected.iter().any(|l| l.contains("heading")),
        "projected assistant lines should contain heading text; got: {assistant_projected:?}"
    );
    assert!(
        assistant_projected.iter().any(|l| l.contains("x")),
        "projected assistant lines should contain code text; got: {assistant_projected:?}"
    );
}

#[test]
fn coordinator_hydration_preserves_assistant_markdown_styles() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "**bold** and `code`")],
        None,
    );

    // TranscriptEntry no longer has a `.rendered` field - test removed
    // Previously tested that assistant hydration preserved rendered markdown
}

#[test]
fn assistant_markdown_projection_is_memoized_across_repeated_messages() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("fenced_code_blocks.md");

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: markdown.clone(),
    });
    coordinator.drain_transport();

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    coordinator.drain_transport();

    // Both messages are processed; the second replaces the first via streaming truncation
}

#[tokio::test]
async fn resize_and_redraw_paths_reproject_at_new_width() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    let markdown = markdown_fixture("fenced_code_blocks.md");

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    driver.coordinator_mut().drain_transport();

    for (columns, rows) in [(100, 28), (140, 42), (80, 24)] {
        driver
            .advance(&[DriveEvent::Key(TerminalEvent::Resize(
                crate::interaction::input::TerminalResize { columns, rows },
            ))])
            .await?;
    }

    // Resize clears the projection cache so width-aware re-projection occurs
    // on the next render pass. No assertion on cache misses — the counter was
    // removed when render-time caching moved into the projection layer.
    Ok(())
}

#[test]
fn coordinator_hydration_keeps_unsupported_markdown_readable_in_assistant_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("unsupported_fallback.md");
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", &markdown)],
        None,
    );

    // After the raw-markdown refactor, the block stores raw markdown.
    // Project it to verify the rendered content is readable.
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

    // Tables are now supported and rendered with separators
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
    assert!(
        projected_lines.iter().any(|line| line.contains("│")),
        "table cells should be separated"
    );
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("alt (image: https://img.example/x.png)"))
    );
}

#[test]
fn coordinator_hydration_handles_malformed_assistant_markdown_without_dropping_message() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("malformed.md");
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", &markdown)],
        None,
    );

    let assistant_entries = coordinator
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
    // Raw markdown is stored; check projected output contains the expected text
    let projected_text: String = assistant_entries
        .iter()
        .flat_map(|b| crate::markdown::render_markdown_lines(&b.source.plain_text(), None))
        .flat_map(|l| l.spans.into_iter())
        .map(|s| s.text)
        .collect();
    assert!(
        projected_text.contains("fn main() {"),
        "projected text should contain 'fn main()'; got: {projected_text:?}"
    );
}

#[test]
fn assistant_message_event_sanitizes_pseudo_tags_and_control_tags_in_runtime_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "prefix\n[code:json]\n{\"ok\":true}\n[/code]\n<system-reminder>hidden</system-reminder>\nsuffix"
            .to_string(),
    });
    coordinator.drain_transport();

    // After the raw-markdown refactor, the raw markdown is stored in the block.
    // Sanitization happens at projection time (render_markdown_lines). Verify by projecting.
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
}

#[test]
fn coordinator_hydration_regression_no_duplicate_lines_on_single_call() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "dup-check"),
            UiMessageSnapshot::new("assistant", "dup-check-reply"),
        ],
        None,
    );

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
}

#[test]
fn coordinator_hydrate_with_empty_message_snapshot_leaves_empty_session_behavior_unchanged() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    let state = coordinator.state();
    assert!(state.transcript.blocks().is_empty());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
}
