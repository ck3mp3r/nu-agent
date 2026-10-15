use super::ir::{
    BlockSource, ContentKind, Display, DisplaySection, Fill, Lane, MessageRole, NoticeKind,
    StyleHint, Tool, ToolName,
};
use super::items::*;
use super::renderer::{ItemStatus, Renderable};
use crate::protocol::tool_args::CallLine;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ── Message ─────────────────────────────────────────────────────────────────

/// A message stores its role and raw markdown, and answers the three
/// `Renderable` questions per role.
#[test]
fn message_stores_role_markdown_and_renderable_shape() {
    // -- Setup & Fixtures
    let user = Message {
        role: MessageRole::User,
        markdown: "# Hello".to_string(),
    };
    let assistant = Message {
        role: MessageRole::Assistant,
        markdown: "**bold**".to_string(),
    };

    // -- Exec & Check: raw markdown and role are stored verbatim.
    assert_eq!(user.role, MessageRole::User);
    assert_eq!(user.markdown, "# Hello");
    assert_eq!(assistant, assistant.clone(), "clone must be equal");

    // -- Check: user carries the marker lane and full fill.
    assert_eq!(user.lane(), Lane::Marker("▏"));
    assert_eq!(user.fill(), Fill::Full);
    assert_eq!(
        user.source(),
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "# Hello".to_string(),
        }
    );

    // -- Check: assistant carries the blank lane and no fill.
    assert_eq!(assistant.lane(), Lane::Blank);
    assert_eq!(assistant.fill(), Fill::None);
    assert_eq!(
        assistant.source(),
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "**bold**".to_string(),
        }
    );
}

// ── Notice / Spacer / Banner ────────────────────────────────────────────────

/// Notice, spacer, and banner each answer the three `Renderable` questions.
#[test]
fn notice_spacer_banner_renderable_table() {
    // -- Setup & Fixtures
    let compaction = Notice {
        kind: NoticeKind::Compaction,
        text: "compacted 5 blocks".to_string(),
    };
    let system = Notice {
        kind: NoticeKind::System,
        text: "system note".to_string(),
    };
    let spacer = Spacer;
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec & Check: compaction notice — tilde marker, no fill, notice source.
    assert_eq!(compaction.lane(), Lane::Marker("~"));
    assert_eq!(compaction.fill(), Fill::None);
    assert_eq!(
        compaction.source(),
        BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compacted 5 blocks".to_string(),
        }
    );

    // -- Check: system notice — dot marker, no fill.
    assert_eq!(system.lane(), Lane::Marker("·"));
    assert_eq!(system.fill(), Fill::None);

    // -- Check: spacer — blank lane, no fill, Spacer source.
    assert!(matches!(spacer.source(), BlockSource::Spacer));
    assert_eq!(spacer.lane(), Lane::Blank);
    assert_eq!(spacer.fill(), Fill::None);

    // -- Check: banner — system-blank lane, no fill, Banner source.
    assert!(matches!(banner.source(), BlockSource::Banner { .. }));
    assert_eq!(banner.lane(), Lane::SystemBlank);
    assert_eq!(banner.fill(), Fill::None);
}

// ── CallLine::from_json_summary / annotate_diff_hint ────────────────────────

/// The JSON fallback summary is arrow-prefixed; `annotate_diff_hint`
/// classifies each diff line role.
#[test]
fn call_line_and_annotate_diff_hint_table() {
    // -- Exec & Check: the JSON fallback summary is arrow-prefixed.
    let line = CallLine::from_json_summary(r#"{"path":"a.rs"}"#);
    assert_eq!(line.summary, r#"→ {"path":"a.rs"}"#);

    // -- Check: annotate_diff_hint classifies each diff line role.
    let cases = [
        ("+added", StyleHint::DiffAdd),
        ("-removed", StyleHint::DiffRemove),
        ("@@ -1,2 +1,2 @@", StyleHint::DiffHunk),
        ("--- a.rs", StyleHint::Meta),
        ("+++ b.rs", StyleHint::Meta),
        ("plain", StyleHint::Normal),
    ];
    for (text, expected) in cases {
        assert_eq!(
            annotate_diff_hint(text),
            expected,
            "{text:?} classified wrong"
        );
    }
}

// ── Tool: source, lane, and fill from preview content kind ──────────────────

/// A tool projects its name and call line, uses the gear marker lane, and
/// fills `Code` only when its preview carries diff or code content.
#[test]
fn tool_source_lane_and_fill_table() -> Result<()> {
    // -- Setup & Fixtures
    let no_preview = Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ a.rs (diff)".to_string(),
        },
        preview: None,
        result: None,
        status: ItemStatus::InProgress,
    };
    let diff_preview = Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: Some("edit a.rs".to_string()),
            sections: vec![DisplaySection {
                label: Some("a.rs".to_string()),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                    path: "test.rs".to_string(),
                },
                content: "diff".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: ItemStatus::InProgress,
    };
    let code_preview = Tool {
        name: ToolName("nu".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: Some("nu".to_string()),
            sections: vec![DisplaySection {
                label: Some("nu".to_string()),
                kind: ContentKind::Code {
                    language: "nu".to_string(),
                },
                content: "ls".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: ItemStatus::InProgress,
    };
    let plain_preview = Tool {
        name: ToolName("custom".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: Some("custom".to_string()),
            sections: vec![DisplaySection {
                label: Some("output".to_string()),
                kind: ContentKind::Plain,
                content: "some result".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: ItemStatus::InProgress,
    };

    // -- Exec & Check: the source carries the tool name and call line.
    let BlockSource::Tool { name, call, .. } = no_preview.source() else {
        return Err("Tool must project to BlockSource::Tool".into());
    };
    assert_eq!(name, ToolName("edit".to_string()));
    assert_eq!(call.summary, "→ a.rs (diff)");

    // -- Check: no preview — no fill, gear marker lane, no preview in source.
    assert_eq!(no_preview.fill(), Fill::None);
    assert_eq!(no_preview.lane(), Lane::Marker("⚙"));
    assert_eq!(
        no_preview.source(),
        BlockSource::Tool {
            name: ToolName("edit".to_string()),
            call: no_preview.call.clone(),
            preview: None,
        }
    );

    // -- Check: diff and code previews fill code; a plain preview does not.
    assert_eq!(diff_preview.fill(), Fill::Code);
    assert_eq!(code_preview.fill(), Fill::Code);
    assert_eq!(plain_preview.fill(), Fill::None);
    Ok(())
}

// ── Display::has_code_or_diff ───────────────────────────────────────────────

/// `has_code_or_diff` is true only for diff or code sections.
#[test]
fn display_has_code_or_diff_table() {
    // -- Setup & Fixtures
    let diff_display = Display {
        title: Some("d".to_string()),
        sections: vec![DisplaySection {
            label: Some("s".to_string()),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let plain_display = Display {
        title: Some("d".to_string()),
        sections: vec![DisplaySection {
            label: Some("s".to_string()),
            kind: ContentKind::Plain,
            content: String::new(),
            stats: None,
        }],
    };
    let empty_display = Display {
        title: Some("d".to_string()),
        sections: vec![],
    };

    // -- Exec & Check
    assert!(diff_display.has_code_or_diff());
    assert!(!plain_display.has_code_or_diff());
    assert!(!empty_display.has_code_or_diff());
}
