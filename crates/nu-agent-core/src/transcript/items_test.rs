use super::ir::{
    BlockSource, ContentKind, Display, DisplaySection, Fill, Lane, MessageRole, NoticeKind,
    StyleHint, Tool, ToolName,
};
use super::items::*;
use super::renderer::Renderable;
use crate::protocol::tool_args::CallLine;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ── Message stores raw markdown ──────────────────────────────────────────────

#[test]
fn message_stores_role_and_raw_markdown() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::User,
        markdown: "# Hello".to_string(),
    };

    // -- Exec & Check
    assert_eq!(msg.role, MessageRole::User);
    assert_eq!(msg.markdown, "# Hello");
}

#[test]
fn message_clone_is_equal() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: "**bold**".to_string(),
    };

    // -- Exec & Check
    assert_eq!(msg, msg.clone());
}

// ── Message: user carries marker lane and full fill ─────────────────────────

#[test]
fn user_message_lanes_marker_and_fills_full() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::User,
        markdown: "hi".to_string(),
    };

    // -- Exec & Check
    assert_eq!(msg.lane(), Lane::Marker("▏"));
    assert_eq!(msg.fill(), Fill::Full);
    assert_eq!(
        msg.source(),
        BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "hi".to_string(),
        }
    );
}

// ── Message: assistant carries blank lane and no fill ────────────────────────

#[test]
fn assistant_message_lanes_blank_and_fills_none() {
    // -- Setup & Fixtures
    let msg = Message {
        role: MessageRole::Assistant,
        markdown: "hello".to_string(),
    };

    // -- Exec & Check
    assert_eq!(msg.lane(), Lane::Blank);
    assert_eq!(msg.fill(), Fill::None);
    assert_eq!(
        msg.source(),
        BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "hello".to_string(),
        }
    );
}

// ── Notice ───────────────────────────────────────────────────────────────────

#[test]
fn notice_compaction_lanes_tilde_marker() {
    // -- Setup & Fixtures
    let notice = Notice {
        kind: NoticeKind::Compaction,
        text: "compacted 5 blocks".to_string(),
    };

    // -- Exec & Check
    assert_eq!(notice.lane(), Lane::Marker("~"));
    assert_eq!(notice.fill(), Fill::None);
    assert_eq!(
        notice.source(),
        BlockSource::Notice {
            kind: NoticeKind::Compaction,
            text: "compacted 5 blocks".to_string(),
        }
    );
}

#[test]
fn notice_system_lanes_dot_marker() {
    // -- Setup & Fixtures
    let notice = Notice {
        kind: NoticeKind::System,
        text: "system note".to_string(),
    };

    // -- Exec & Check
    assert_eq!(notice.lane(), Lane::Marker("·"));
    assert_eq!(notice.fill(), Fill::None);
}

// ── Spacer / Banner ──────────────────────────────────────────────────────────

#[test]
fn spacer_is_blank_lane_with_no_fill() {
    // -- Setup & Fixtures
    let spacer = Spacer;

    // -- Exec & Check
    assert!(matches!(spacer.source(), BlockSource::Spacer));
    assert_eq!(spacer.lane(), Lane::Blank);
    assert_eq!(spacer.fill(), Fill::None);
}

#[test]
fn banner_is_system_blank_lane_with_no_fill() {
    // -- Setup & Fixtures
    let banner = Banner {
        text: "line1\nline2".to_string(),
    };

    // -- Exec & Check
    assert!(matches!(banner.source(), BlockSource::Banner { .. }));
    assert_eq!(banner.lane(), Lane::SystemBlank);
    assert_eq!(banner.fill(), Fill::None);
}

// ── CallLine::from_json_summary ──────────────────────────────────────────────

#[test]
fn call_line_from_json_summary_prefixes_arrow() {
    // -- Exec & Check
    let line = crate::protocol::tool_args::CallLine::from_json_summary(r#"{"path":"a.rs"}"#);
    assert_eq!(line.summary, r#"→ {"path":"a.rs"}"#);
}

// ── annotate_diff_hint ───────────────────────────────────────────────────────

#[test]
fn annotate_diff_hint_identifies_plus_lines() {
    assert_eq!(annotate_diff_hint("+added"), StyleHint::DiffAdd);
}

#[test]
fn annotate_diff_hint_identifies_minus_lines() {
    assert_eq!(annotate_diff_hint("-removed"), StyleHint::DiffRemove);
}

#[test]
fn annotate_diff_hint_identifies_hunk_lines() {
    assert_eq!(annotate_diff_hint("@@ -1,2 +1,2 @@"), StyleHint::DiffHunk);
}

#[test]
fn annotate_diff_hint_identifies_file_headers() {
    assert_eq!(annotate_diff_hint("--- a.rs"), StyleHint::Meta);
    assert_eq!(annotate_diff_hint("+++ b.rs"), StyleHint::Meta);
}

#[test]
fn annotate_diff_hint_returns_normal_for_plain() {
    assert_eq!(annotate_diff_hint("plain"), StyleHint::Normal);
}

// ── Tool: fill from preview content kind ────────────────────────────────────

#[test]
fn tool_name_is_edit_is_true_only_for_the_edit_builtin() {
    // -- Exec & Check: typed identity from the built-in name table.
    assert!(ToolName("edit".to_string()).is_edit());
    assert!(!ToolName("read".to_string()).is_edit());
    assert!(!ToolName("nu".to_string()).is_edit());
    // A title-like string is not a tool name; never a prefix probe.
    assert!(!ToolName("edit notes/todo.md".to_string()).is_edit());
    assert!(!ToolName(String::new()).is_edit());

    // -- Exec & Check: `is_nu` is the same typed identity, for the `nu`
    // built-in only.
    assert!(ToolName("nu".to_string()).is_nu());
    assert!(!ToolName("edit".to_string()).is_nu());
    assert!(!ToolName("read".to_string()).is_nu());
    assert!(!ToolName("edit notes/todo.md".to_string()).is_nu());
    assert!(!ToolName(String::new()).is_nu());
}

#[test]
fn tool_source_carries_tool_name() -> Result<()> {
    // -- Setup & Fixtures
    let tool = Tool {
        name: ToolName("grep".to_string()),
        call: CallLine {
            summary: "→ pattern in src".to_string(),
        },
        preview: None,
        result: None,
        status: super::renderer::ItemStatus::InProgress,
    };

    // -- Exec
    let source = tool.source();

    // -- Check
    let BlockSource::Tool { name, call, .. } = source else {
        return Err("Tool must project to BlockSource::Tool".into());
    };
    assert_eq!(name, ToolName("grep".to_string()));
    assert_eq!(call.summary, "→ pattern in src");
    Ok(())
}

#[test]
fn tool_without_preview_fills_none() {
    // -- Setup & Fixtures
    let tool = Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: "→ a.rs (diff)".to_string(),
        },
        preview: None,
        result: None,
        status: super::renderer::ItemStatus::InProgress,
    };

    // -- Exec & Check
    assert_eq!(tool.fill(), Fill::None);
    assert_eq!(tool.lane(), Lane::Marker("⚙"));
    assert_eq!(
        tool.source(),
        BlockSource::Tool {
            name: ToolName("edit".to_string()),
            call: tool.call.clone(),
            preview: None,
        }
    );
}

#[test]
fn tool_with_diff_preview_fills_code() {
    // -- Setup & Fixtures
    let tool = Tool {
        name: ToolName("edit".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: "edit a.rs".to_string(),
            sections: vec![DisplaySection {
                label: "a.rs".to_string(),
                kind: ContentKind::Diff {
                    language: "diff".to_string(),
                },
                content: "diff".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: super::renderer::ItemStatus::InProgress,
    };

    // -- Exec & Check
    assert_eq!(tool.fill(), Fill::Code);
}

#[test]
fn tool_with_code_preview_fills_code() {
    // -- Setup & Fixtures
    let tool = Tool {
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
                content: "ls".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: super::renderer::ItemStatus::InProgress,
    };

    // -- Exec & Check
    assert_eq!(tool.fill(), Fill::Code);
}

#[test]
fn tool_with_plain_preview_fills_none() {
    // -- Setup & Fixtures
    let tool = Tool {
        name: ToolName("custom".to_string()),
        call: CallLine {
            summary: String::new(),
        },
        preview: Some(Display {
            title: "custom".to_string(),
            sections: vec![DisplaySection {
                label: "output".to_string(),
                kind: ContentKind::Plain,
                content: "some result".to_string(),
                stats: None,
            }],
        }),
        result: None,
        status: super::renderer::ItemStatus::InProgress,
    };

    // -- Exec & Check
    assert_eq!(tool.fill(), Fill::None);
}

#[test]
fn display_has_code_or_diff_true_for_diff_and_code_only() {
    // -- Setup & Fixtures
    let diff_display = Display {
        title: "d".to_string(),
        sections: vec![DisplaySection {
            label: "s".to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: String::new(),
            stats: None,
        }],
    };
    let plain_display = Display {
        title: "d".to_string(),
        sections: vec![DisplaySection {
            label: "s".to_string(),
            kind: ContentKind::Plain,
            content: String::new(),
            stats: None,
        }],
    };
    let empty_display = Display {
        title: "d".to_string(),
        sections: vec![],
    };

    // -- Exec & Check
    assert!(diff_display.has_code_or_diff());
    assert!(!plain_display.has_code_or_diff());
    assert!(!empty_display.has_code_or_diff());
}

// ── Notice: both kinds (pre-existing, kept as regression guard) ─────────────

#[test]
fn notice_compaction_lanes_tilde_marker_regression() {
    // -- Setup & Fixtures
    let notice = Notice {
        kind: NoticeKind::Compaction,
        text: "compacted".to_string(),
    };

    // -- Exec & Check
    assert_eq!(notice.lane(), Lane::Marker("~"));
    assert_eq!(notice.fill(), Fill::None);
}
