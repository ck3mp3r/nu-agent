use super::ir::*;
use super::items::{Banner, Message, Spacer};
use super::renderer::{ItemStatus, Renderable};
use crate::protocol::tool_args::CallLine;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ── Block / BlockSource ──────────────────────────────────────────────────────

#[test]
fn block_construction_stores_source_lane_fill_status() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };

    // -- Exec
    let block = Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    };

    // -- Check
    assert_eq!(
        block.source,
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(block.lane, Lane::Marker("▏"));
    assert_eq!(block.fill, Fill::Full);
    assert!(block.status.is_none());
}

#[test]
fn block_holds_tool_source_with_call_line() -> Result<()> {
    // -- Setup & Fixtures
    let call = CallLine {
        summary: "→ {\"path\":\"a.rs\"}".to_string(),
    };

    // -- Exec
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("read".to_string()),
            call,
            preview: None,
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: Some(ItemStatus::InProgress),
    };

    // -- Check
    let BlockSource::Tool {
        name,
        call,
        preview,
    } = &block.source
    else {
        return Err("expected Tool source".into());
    };
    assert_eq!(name.0, "read");
    assert_eq!(call.summary, "→ {\"path\":\"a.rs\"}");
    assert!(preview.is_none());
    assert_eq!(block.status, Some(ItemStatus::InProgress));
    Ok(())
}

#[test]
fn block_holds_notice_banner_and_spacer_sources() {
    // -- Setup & Fixtures
    let sources = [
        BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compacted".to_string(),
        },
        BlockSource::Banner {
            text: "logo".to_string(),
        },
        BlockSource::Spacer,
    ];

    // -- Exec & Check
    assert!(
        matches!(
            sources[0],
            BlockSource::Notice {
                kind: NoticeKind::Compaction,
                ..
            }
        ),
        "first source must be a Compaction notice"
    );
    assert!(
        matches!(sources[1], BlockSource::Banner { .. }),
        "second source must be a Banner"
    );
    assert!(
        matches!(sources[2], BlockSource::Spacer),
        "third source must be a Spacer"
    );
}

// ── BlockFamily::is_tool_family ─────────────────────────────────────────────

/// `Tool` and `ToolDisplay` are the tool family; every other variant is not.
#[test]
fn block_family_is_tool_family_covers_every_variant() {
    // -- Exec & Check
    assert!(BlockFamily::Tool.is_tool_family(), "Tool is a tool family");
    assert!(
        BlockFamily::ToolDisplay.is_tool_family(),
        "ToolDisplay is a tool family"
    );
    assert!(!BlockFamily::User.is_tool_family(), "User is not");
    assert!(!BlockFamily::Assistant.is_tool_family(), "Assistant is not");
    assert!(!BlockFamily::Notice.is_tool_family(), "Notice is not");
    assert!(!BlockFamily::Banner.is_tool_family(), "Banner is not");
}

// ── Message Renderable impl ──────────────────────────────────────────────────

#[test]
fn message_user_lanes_marker_and_fills_full() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };

    // -- Exec & Check
    assert_eq!(
        msg.source(),
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(msg.lane(), Lane::Marker("▏"));
    assert_eq!(msg.fill(), Fill::Full);
}

#[test]
fn message_assistant_is_blank_lane_with_no_fill() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: "hi".to_string(),
    };

    // -- Exec & Check
    assert_eq!(
        msg.source(),
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hi".to_string(),
        }
    );
    assert_eq!(msg.lane(), Lane::Blank);
    assert_eq!(msg.fill(), Fill::None);
}

// ── Spacer / Banner Renderable impls ────────────────────────────────────────

#[test]
fn spacer_source_is_spacer_with_blank_lane_and_no_fill() {
    // -- Setup & Fixtures
    let spacer = Spacer;

    // -- Exec & Check
    assert!(matches!(spacer.source(), BlockSource::Spacer));
    assert_eq!(spacer.lane(), Lane::Blank);
    assert_eq!(spacer.fill(), Fill::None);
}

#[test]
fn banner_source_is_blank_system_lane_with_no_fill() {
    // -- Setup & Fixtures
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec & Check
    assert_eq!(
        banner.source(),
        BlockSource::Banner {
            text: "line1\nline2".to_string()
        }
    );
    assert_eq!(banner.lane(), Lane::SystemBlank);
    assert_eq!(banner.fill(), Fill::None);
}

// ── Content ──────────────────────────────────────────────────────────────────

#[test]
fn content_wraps_content_lines() {
    // -- Setup & Fixtures
    let content = Content {
        lines: vec![ContentLine::single("x".to_string(), StyleHint::Normal)],
    };

    // -- Check
    assert_eq!(content.lines.len(), 1);
    assert_eq!(content.lines[0].spans[0].text, "x");
}

// ── Display / DisplaySection / ContentKind ──────────────────────────────────

#[test]
fn display_stores_title_and_sections() {
    // -- Setup & Fixtures
    let display = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "@@ -1 +1 @@".to_string(),
            stats: None,
        }],
    };

    // -- Check
    assert_eq!(display.title, "edit a.rs");
    assert_eq!(display.sections.len(), 1);
    assert_eq!(display.sections[0].label, "a.rs");
    assert_eq!(display.sections[0].content, "@@ -1 +1 @@");
}

/// `is_single_diff_section` is a pure shape query: exactly one section and it
/// is a diff. One code section, one plain section, or two sections are false.
#[test]
fn display_is_single_diff_section_is_a_pure_shape_query() {
    // -- Setup & Fixtures
    let one_diff = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_code = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_plain = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Plain,
            content: String::new(),
            stats: None,
        }],
    };
    let two_diffs = Display {
        title: "t".to_string(),
        sections: vec![
            DisplaySection {
                label: "a".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: String::new(),
                stats: None,
            },
            DisplaySection {
                label: "b".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: String::new(),
                stats: None,
            },
        ],
    };
    let no_sections = Display {
        title: "t".to_string(),
        sections: vec![],
    };

    // -- Exec & Check
    assert!(one_diff.is_single_diff_section());
    assert!(!one_code.is_single_diff_section());
    assert!(!one_plain.is_single_diff_section());
    assert!(!two_diffs.is_single_diff_section());
    assert!(!no_sections.is_single_diff_section());
}

/// `is_single_code_section` is the sibling pure shape query: exactly one
/// section and it is code. One diff section, one plain section, or two
/// sections are false.
#[test]
fn display_is_single_code_section_is_a_pure_shape_query() {
    // -- Setup & Fixtures
    let one_code = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_diff = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let one_plain = Display {
        title: "t".to_string(),
        sections: vec![DisplaySection {
            label: "l".to_string(),
            kind: ContentKind::Plain,
            content: String::new(),
            stats: None,
        }],
    };
    let two_codes = Display {
        title: "t".to_string(),
        sections: vec![
            DisplaySection {
                label: "a".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: String::new(),
                stats: None,
            },
            DisplaySection {
                label: "b".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: String::new(),
                stats: None,
            },
        ],
    };
    let no_sections = Display {
        title: "t".to_string(),
        sections: vec![],
    };

    // -- Exec & Check
    assert!(one_code.is_single_code_section());
    assert!(!one_diff.is_single_code_section());
    assert!(!one_plain.is_single_code_section());
    assert!(!two_codes.is_single_code_section());
    assert!(!no_sections.is_single_code_section());
}

#[test]
fn content_kind_language_returns_language_for_diff_and_code() {
    // -- Exec & Check
    assert_eq!(
        ContentKind::Diff {
            language: "diff".to_string()
        }
        .language(),
        "diff"
    );
    assert_eq!(
        ContentKind::Code {
            language: "nu".to_string()
        }
        .language(),
        "nu"
    );
}

#[test]
fn content_kind_language_returns_empty_for_plain() {
    assert_eq!(ContentKind::Plain.language(), "");
}

#[test]
fn content_kind_is_comparable() {
    // -- Setup & Fixtures
    let kind = ContentKind::Plain;

    // -- Exec & Check
    assert_eq!(kind, ContentKind::Plain);
    assert_ne!(
        kind,
        ContentKind::Code {
            language: "nu".to_string()
        }
    );
}

// ── DisplayStats::format_stats_line ──────────────────────────────────────

/// A stats record with every field `None` formats to no line.
#[test]
fn stats_format_line_all_none_returns_none() {
    // -- Setup & Fixtures
    let stats = DisplayStats::default();

    // -- Exec & Check
    assert!(
        stats.format_stats_line().is_none(),
        "empty stats must format to None"
    );
}

/// Present counters format in fixed order, space-separated.
#[test]
fn stats_format_line_formats_present_fields() -> Result<()> {
    // -- Setup & Fixtures
    let stats = DisplayStats {
        files_changed: Some(2),
        insertions: Some(5),
        deletions: Some(1),
        diff_truncated: None,
        omitted_files: None,
        omitted_hunks: None,
    };

    // -- Exec
    let line = stats.format_stats_line();

    // -- Check
    assert_eq!(line.as_deref(), Some("files=2 +5 -1"));
    Ok(())
}

/// Truncation renders only when `Some(true)`; omissions render their counts.
#[test]
fn stats_format_line_renders_truncation_and_omissions() -> Result<()> {
    // -- Setup & Fixtures
    let stats = DisplayStats {
        files_changed: Some(3),
        insertions: Some(10),
        deletions: Some(4),
        diff_truncated: Some(true),
        omitted_files: Some(1),
        omitted_hunks: Some(2),
    };

    // -- Exec
    let line = stats.format_stats_line();

    // -- Check
    assert_eq!(
        line.as_deref(),
        Some("files=3 +10 -4 truncated=true omitted=1 hunks_omitted=2")
    );
    Ok(())
}

/// `Some(false)` truncation carries no value and must not render.
#[test]
fn stats_format_line_false_truncation_is_omitted() {
    // -- Setup & Fixtures: only a `false` flag — no renderable value.
    let stats = DisplayStats {
        diff_truncated: Some(false),
        ..DisplayStats::default()
    };

    // -- Exec & Check
    assert!(
        stats.format_stats_line().is_none(),
        "Some(false) truncation is not a value; got {:?}",
        stats.format_stats_line()
    );
}

// ── span / content_line (pre-existing) ──────────────────────────────────────

#[test]
fn span_normal_constructor_sets_hint() {
    let span = Span::normal("x".to_string());
    assert_eq!(
        span,
        Span {
            text: "x".to_string(),
            hint: StyleHint::Normal
        }
    );
}

#[test]
fn span_new_stores_arbitrary_hint() {
    let span = Span::new("y".to_string(), StyleHint::DiffAdd);
    assert_eq!(span.text, "y");
    assert_eq!(span.hint, StyleHint::DiffAdd);
}

#[test]
fn content_line_single_creates_one_span() {
    let line = ContentLine::single("hello".to_string(), StyleHint::Emphasis);
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].text, "hello");
    assert_eq!(line.spans[0].hint, StyleHint::Emphasis);
}

#[test]
fn content_line_empty_has_no_spans() {
    assert!(ContentLine::empty().spans.is_empty());
}

#[test]
fn content_line_from_spans_preserves_order() {
    let spans = vec![
        Span::normal("a".to_string()),
        Span::meta("b".to_string()),
        Span::muted("c".to_string()),
    ];
    let line = ContentLine::from_spans(spans.clone());
    assert_eq!(line.spans, spans);
}

// ── BlockSource::plain_text ──────────────────────────────────────────────────

#[test]
fn block_source_plain_text_flattens_each_variant() -> Result<()> {
    // -- Setup & Fixtures
    let sources = [
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hello world".to_string(),
        },
        BlockSource::Tool {
            name: ToolName("read".to_string()),
            call: CallLine {
                summary: "→ read a.rs".to_string(),
            },
            preview: None,
        },
        BlockSource::ToolDisplay {
            lines: vec![
                ContentLine::from_spans(vec![
                    Span::normal("diff ".to_string()),
                    Span::muted("a.rs".to_string()),
                ]),
                ContentLine::single("context".to_string(), StyleHint::Normal),
            ],
        },
        BlockSource::Notice {
            kind: NoticeKind::System,
            text: "notice text".to_string(),
        },
        BlockSource::Banner {
            text: "banner".to_string(),
        },
        BlockSource::Spacer,
    ];
    let expected = [
        "hello world".to_string(),
        "→ read a.rs".to_string(),
        "diff a.rs\ncontext".to_string(),
        "notice text".to_string(),
        "banner".to_string(),
        String::new(),
    ];

    // -- Exec & Check
    for (source, expected) in sources.iter().zip(expected.iter()) {
        assert_eq!(
            &source.plain_text(),
            expected,
            "variant {source:?} flattened wrong"
        );
    }
    Ok(())
}

// ── Fill::from_preview ──────────────────────────────────────────────────────

#[test]
fn fill_from_preview_none_returns_none() {
    // -- Exec & Check
    assert_eq!(Fill::from_preview(&None), Fill::None);
}

#[test]
fn fill_from_preview_plain_only_display_returns_none() -> Result<()> {
    // -- Setup & Fixtures
    let display = Display {
        title: "plain output".to_string(),
        sections: vec![DisplaySection {
            label: "out".to_string(),
            kind: ContentKind::Plain,
            content: "line".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert_eq!(Fill::from_preview(&Some(display)), Fill::None);
    Ok(())
}

#[test]
fn fill_from_preview_diff_section_returns_code() -> Result<()> {
    // -- Setup & Fixtures
    let display = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "@@ -1 +1 @@".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert_eq!(Fill::from_preview(&Some(display)), Fill::Code);
    Ok(())
}

// ── BlockSource::project ────────────────────────────────────────────────────

/// Markdown projects through the shared pulldown-cmark pipeline at the
/// canvas width — identical to calling the helper directly.
#[test]
fn project_markdown_matches_render_markdown_lines() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Markdown {
        role: MessageRole::Assistant,
        markdown: "# Title\n\nbody **bold**".to_string(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let expected =
        crate::transcript::markdown::render_markdown_lines("# Title\n\nbody **bold**", Some(80));
    assert_eq!(
        projected, expected,
        "markdown must project via the shared pipeline"
    );
    assert!(!projected.is_empty(), "markdown must produce lines");
    Ok(())
}

/// The tool call line carries the name in emphasis and the summary in muted.
#[test]
fn project_tool_call_line_has_emphasis_name_and_muted_summary() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs".to_string(),
        },
        preview: None,
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let row0 = projected.first().ok_or("row 0 must exist")?;
    assert_eq!(row0.spans.len(), 2, "name + summary spans");
    assert_eq!(row0.spans[0].text, "edit");
    assert_eq!(row0.spans[0].hint, StyleHint::Emphasis);
    assert_eq!(row0.spans[1].text, " → foo.rs");
    assert_eq!(row0.spans[1].hint, StyleHint::Muted);
    Ok(())
}

/// A nameless tool block projects the summary span alone.
#[test]
fn project_nameless_tool_renders_summary_span_only() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName(String::new()),
        call: CallLine {
            summary: "→ foo.rs".to_string(),
        },
        preview: None,
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let row0 = projected.first().ok_or("row 0 must exist")?;
    assert_eq!(row0.spans.len(), 1, "nameless tool renders one span");
    assert_eq!(row0.spans[0].text, "→ foo.rs");
    assert_eq!(row0.spans[0].hint, StyleHint::Muted);
    Ok(())
}

/// A diff preview section projects through the diff helper; a code preview
/// section through the code-block helper.
#[test]
fn project_tool_diff_and_code_preview_sections() -> Result<()> {
    // -- Setup & Fixtures
    let diff_display = Display {
        title: "edit a.rs".to_string(),
        sections: vec![DisplaySection {
            label: "a.rs".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
            stats: None,
        }],
    };
    let code_display = Display {
        title: "nu".to_string(),
        sections: vec![DisplaySection {
            label: "nu".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls | select name".to_string(),
            stats: None,
        }],
    };

    // -- Exec
    let diff_source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ a.rs (diff)".to_string(),
        },
        preview: Some(diff_display),
    };
    let diff_projected = diff_source.project(80);
    let code_source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls | select name".to_string(),
        },
        preview: Some(code_display),
    };
    let code_projected = code_source.project(80);

    // -- Check: call line first, then helper-projected preview lines.
    // The diff display is a single-diff-section `edit`, so its title and
    // section label are suppressed: the diff lines follow the call line
    // directly.
    let expected_diff = crate::transcript::markdown::project_diff_lines(
        "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
    );
    assert_eq!(diff_projected[1..], expected_diff[..], "diff preview lines");
    // The code display is a `nu` tool with a single code section: its title
    // and section label are suppressed, so the highlighted command lines
    // follow the call line directly.
    let expected_code =
        crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(code_projected[1..], expected_code[..], "code preview lines");
    Ok(())
}

/// A plain preview section projects to a single Normal content line.
#[test]
fn project_tool_plain_preview_section_is_normal_line() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Tool {
        name: ToolName("tool".to_string()),
        call: CallLine {
            summary: "→ out".to_string(),
        },
        preview: Some(Display {
            title: "plain".to_string(),
            sections: vec![DisplaySection {
                label: "out".to_string(),
                kind: ContentKind::Plain,
                content: "done".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: call line, title, label, then the plain content line. The
    // `tool` name is not `edit`, so no suppression applies.
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[0], "tool → out", "call line");
    assert_eq!(lines[1], "plain", "title row");
    assert_eq!(lines[2], "out ()", "label row (Plain has no language)");
    assert_eq!(lines[3], "done", "content row");
    let preview_line = projected.get(3).ok_or("preview line must exist")?;
    assert_eq!(preview_line.spans.len(), 1);
    assert_eq!(preview_line.spans[0].hint, StyleHint::Normal);
    Ok(())
}

/// Flatten projected lines to per-line plain text for whole-line assertions.
/// Whole-line checks avoid false matches against the call-summary substring
/// ("→ foo.rs (diff)").
fn projected_lines_text(lines: &[ContentLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect()
}

/// WHEN `project` renders a Tool preview Display titled "edit foo.rs", THE
/// output SHALL contain the title text.
#[test]
fn project_tool_preview_includes_display_title() -> Result<()> {
    // -- Setup & Fixtures: a non-edit, non-nu tool with a single code section
    // (so neither the edit-title nor the nu-title suppression applies).
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: a whole line equals the title.
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "edit foo.rs"),
        "display title must render as its own line, got: {lines:?}"
    );
    Ok(())
}

/// WHEN `project` renders a Tool preview DisplaySection labeled "alpha.rs" with
/// `ContentKind::Diff`, THE output SHALL contain a line "alpha.rs (diff)".
#[test]
fn project_tool_preview_includes_section_label_with_language() -> Result<()> {
    // -- Setup & Fixtures: two sections so the single-section edit suppression
    // does not apply; the label differs from the call summary so a whole-line
    // check cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![
                DisplaySection {
                    label: "alpha.rs".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- a\n+++ b\n".to_string(),
                    stats: None,
                },
                DisplaySection {
                    label: "beta.rs".to_string(),
                    kind: ContentKind::Diff {
                        language: "diff".to_string(),
                    },
                    content: "--- c\n+++ d\n".to_string(),
                    stats: None,
                },
            ],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "alpha.rs (diff)"),
        "section label must render as `label (language)`, got: {lines:?}"
    );
    Ok(())
}

/// WHEN `project` renders a Tool preview DisplaySection with stats, THE
/// output SHALL contain the stats line.
#[test]
fn project_tool_preview_includes_stats_line() -> Result<()> {
    // -- Setup & Fixtures: a non-edit, non-nu tool with a single code section
    // (no stats suppression).
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "nu".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
                stats: Some(DisplayStats {
                    files_changed: Some(2),
                    insertions: Some(5),
                    deletions: Some(1),
                    diff_truncated: None,
                    omitted_files: None,
                    omitted_hunks: None,
                }),
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("files=2") && l.contains("+5") && l.contains("-1")),
        "stats line must render, got: {lines:?}"
    );
    Ok(())
}

/// WHEN the preview is a single-diff-section for an edit tool, THE title line
/// and the section label line SHALL be suppressed.
#[test]
fn project_tool_single_diff_edit_suppresses_title_and_label() -> Result<()> {
    // -- Setup & Fixtures: the label row would render as "changes (diff)",
    // which is distinct from the call summary "→ foo.rs (diff)", so a
    // whole-line check cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![DisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: neither the title line nor the label line appears.
    let lines = projected_lines_text(&projected);
    assert!(
        !lines.iter().any(|l| l == "edit foo.rs"),
        "redundant edit title must be suppressed, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "changes (diff)"),
        "redundant edit section label must be suppressed, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("+++ b")),
        "the diff content must still render, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a non-edit tool carries a single-diff-section preview, THE title and
/// label SHALL NOT be suppressed: suppression is keyed on TYPED tool identity
/// plus display shape, so display shape alone never triggers it.
#[test]
fn project_tool_non_edit_with_single_diff_preview_keeps_title_and_label() -> Result<()> {
    // -- Setup & Fixtures: an `edit`-shaped display (single diff section,
    // title text starting with "edit ") on a NON-edit tool. Shape and title
    // text must not influence the decision.
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "→ foo.rs (diff)".to_string(),
        },
        preview: Some(Display {
            title: "edit foo.rs".to_string(),
            sections: vec![DisplaySection {
                label: "changes".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "--- a\n+++ b\n".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: both the title line and the label line render.
    let lines = projected_lines_text(&projected);
    assert!(
        lines.iter().any(|l| l == "edit foo.rs"),
        "non-edit tool must keep its title, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "changes (diff)"),
        "non-edit tool must keep its section label, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a `nu` tool carries a single-code-section preview, THE title row, the
/// section label row, and the stats row SHALL be suppressed: the call line
/// already shows the command, so `nu` / `nu (nu)` / stats add nothing.
#[test]
fn project_tool_single_code_nu_suppresses_title_label_and_stats() -> Result<()> {
    // -- Setup & Fixtures: the label row would render as "nu (nu)" and the
    // title as "nu", both distinct from the call summary "ls | select name",
    // so whole-line checks cannot be satisfied by the summary line.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls | select name".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls | select name".to_string(),
                stats: Some(DisplayStats {
                    files_changed: Some(2),
                    insertions: Some(5),
                    deletions: Some(1),
                    diff_truncated: None,
                    omitted_files: None,
                    omitted_hunks: None,
                }),
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: only the call line, then the highlighted command lines.
    let expected = crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(
        projected[1..],
        expected[..],
        "code lines follow the call line"
    );
    let lines = projected_lines_text(&projected);
    assert!(
        !lines.iter().any(|l| l == "nu"),
        "redundant nu title must be suppressed, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "nu (nu)"),
        "redundant nu section label must be suppressed, got: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("files=2")),
        "nu stats row must be suppressed, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a `nu` tool carries an empty call summary and a single-code-section
/// preview, THE row 0 SHALL render the tool name alone with no trailing
/// space, followed by the highlighted command lines.
#[test]
fn project_tool_empty_summary_nu_renders_name_alone() -> Result<()> {
    // -- Setup & Fixtures: the production shape after the fix — the command
    // lives in the preview, the call line carries no summary.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls | select name".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check: row 0 is exactly "nu" (no trailing space), then the code lines.
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[0], "nu", "row 0 must be the tool name alone");
    let expected = crate::transcript::markdown::project_code_block_lines("nu", "ls | select name");
    assert_eq!(
        projected[1..],
        expected[..],
        "code lines follow the call line"
    );
    Ok(())
}

/// WHEN a `nu` tool carries a preview with more than one section, THE title
/// and every section label SHALL render: suppression is keyed on the
/// single-code-section shape, so a multi-section nu display keeps its rows.
#[test]
fn project_tool_multi_section_nu_keeps_title_and_labels() -> Result<()> {
    // -- Setup & Fixtures: two code sections, so the single-section shape
    // query is false.
    let source = BlockSource::Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: "ls".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![
                DisplaySection {
                    label: "first".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "ls".to_string(),
                    stats: None,
                },
                DisplaySection {
                    label: "second".to_string(),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: "pwd".to_string(),
                    stats: None,
                },
            ],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[1], "nu", "title row must render");
    assert!(
        lines.iter().any(|l| l == "first (nu)"),
        "first section label must render, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "second (nu)"),
        "second section label must render, got: {lines:?}"
    );
    Ok(())
}

/// WHEN a non-nu tool carries a single-code-section preview, THE title and
/// label SHALL NOT be suppressed: suppression is keyed on TYPED tool identity
/// plus display shape, so display shape alone never triggers it.
#[test]
fn project_tool_non_nu_with_single_code_preview_keeps_title_and_label() -> Result<()> {
    // -- Setup & Fixtures: a `nu`-shaped display (single code section, title
    // "nu") on a non-nu tool. Shape and title text must not influence the
    // decision.
    let source = BlockSource::Tool {
        name: ToolName("run".to_string()),
        call: CallLine {
            summary: "ls".to_string(),
        },
        preview: Some(Display {
            title: "nu".to_string(),
            sections: vec![DisplaySection {
                label: "nu".to_string(),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
                stats: None,
            }],
        }),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    let lines = projected_lines_text(&projected);
    assert_eq!(lines[1], "nu", "title row must render");
    assert_eq!(lines[2], "nu (nu)", "label row must render");
    Ok(())
}

/// A notice projects to a single line styled with the Normal hint (the lane
/// role style), matching the pre-refactor `SystemMessage` styling.
#[test]
fn project_notice_is_single_normal_line() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Notice {
        kind: NoticeKind::Compaction,
        text: "compacted 5 blocks".to_string(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].spans.len(), 1);
    assert_eq!(projected[0].spans[0].text, "compacted 5 blocks");
    assert_eq!(projected[0].spans[0].hint, StyleHint::Normal);
    Ok(())
}

/// A banner projects one Normal line per source line.
#[test]
fn project_banner_is_one_normal_line_per_source_line() -> Result<()> {
    // -- Setup & Fixtures
    let source = BlockSource::Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].spans[0].text, "line1");
    assert_eq!(projected[1].spans[0].text, "line2");
    assert!(
        projected
            .iter()
            .all(|l| l.spans[0].hint == StyleHint::Normal),
        "banner lines are Normal"
    );
    Ok(())
}

/// A spacer projects to exactly one empty content line.
#[test]
fn project_spacer_is_one_empty_line() {
    // -- Exec
    let projected = BlockSource::Spacer.project(80);

    // -- Check
    assert_eq!(projected, vec![ContentLine::empty()]);
}

/// A tool display projects to a clone of its stored lines.
#[test]
fn project_tool_display_clones_lines() {
    // -- Setup & Fixtures
    let lines = vec![
        ContentLine::single("diff a.rs".to_string(), StyleHint::DiffAdd),
        ContentLine::single("context".to_string(), StyleHint::Normal),
    ];
    let source = BlockSource::ToolDisplay {
        lines: lines.clone(),
    };

    // -- Exec
    let projected = source.project(80);

    // -- Check
    assert_eq!(
        projected, lines,
        "ToolDisplay must clone its lines verbatim"
    );
}

#[test]
fn fill_from_preview_code_section_returns_code() -> Result<()> {
    // -- Setup & Fixtures
    let display = Display {
        title: "run nu".to_string(),
        sections: vec![DisplaySection {
            label: "script".to_string(),
            kind: ContentKind::Code {
                language: "nu".to_string(),
            },
            content: "ls".to_string(),
            stats: None,
        }],
    };

    // -- Exec & Check
    assert_eq!(Fill::from_preview(&Some(display)), Fill::Code);
    Ok(())
}
