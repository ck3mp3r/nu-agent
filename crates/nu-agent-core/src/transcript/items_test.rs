use super::ir::*;
use super::items::*;
use super::renderer::ItemStatus;
use crate::protocol::tool_args::CallLineRender;

// ── ProseMessage stores raw markdown ─────────────────────────────────────────

#[test]
fn prose_message_stores_raw_markdown() {
    let msg = ProseMessage {
        markdown: "# Hello".to_string(),
    };
    assert_eq!(msg.markdown, "# Hello");
}

#[test]
fn prose_message_clone_is_equal() {
    let msg = ProseMessage {
        markdown: "**bold**".to_string(),
    };
    assert_eq!(msg, msg.clone());
}

// ── to_render_block: User / Assistant carry markdown field ───────────────────

#[test]
fn user_message_produces_user_role_block_with_markdown() {
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
    .to_render_block();
    assert_eq!(block.role, Role::User);
    // Lines are empty — projection happens at render time in TuiRenderer
    assert!(block.lines.is_empty());
    assert_eq!(block.markdown.as_deref(), Some("hi"));
}

#[test]
fn assistant_chunk_produces_assistant_role_block_with_markdown() {
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: "hello".to_string(),
        }),
        status: None,
    }
    .to_render_block();
    assert_eq!(block.role, Role::Assistant);
    assert!(block.lines.is_empty());
    assert_eq!(block.markdown.as_deref(), Some("hello"));
}

// ── Non-prose blocks have markdown: None ─────────────────────────────────────

#[test]
fn tool_invocation_other_tool_produces_three_spans() {
    // -- Setup & Fixtures
    let block = ToolInvocation {
        name: "run".to_string(),
        source: "builtin".to_string(),
        call_line: CallLineRender::Inline {
            summary: "→ {\"cmd\":\"ls\"}".to_string(),
        },
    }
    .to_render_block();

    // -- Check
    assert_eq!(block.role, Role::Tool);
    assert!(block.markdown.is_none());
    assert_eq!(block.lines[0].spans.len(), 3);
    assert_eq!(block.lines[0].spans[0], Span::emphasis("run".to_string()));
    assert_eq!(block.lines[0].spans[1], Span::meta("builtin".to_string()));
    assert_eq!(
        block.lines[0].spans[2],
        Span::muted(" → {\"cmd\":\"ls\"}".to_string())
    );
}

#[test]
fn tool_invocation_nu_renders_status_row_and_code_block() {
    // -- Setup & Fixtures
    let block = ToolInvocation {
        name: "nu".to_string(),
        source: "builtin".to_string(),
        call_line: CallLineRender::CodeBlock {
            language: "nu".to_string(),
            code: "ls | select name type size".to_string(),
        },
    }
    .to_render_block();

    // -- Check
    assert_eq!(block.role, Role::Tool);
    assert!(block.markdown.is_none());

    // Status row: name + source only, no command text, no arrow.
    assert_eq!(block.lines.len(), 2, "status row + one code row");
    assert_eq!(block.lines[0].spans.len(), 2, "name + source only");
    assert_eq!(block.lines[0].spans[0], Span::emphasis("nu".to_string()));
    assert_eq!(block.lines[0].spans[1], Span::meta("builtin".to_string()));
    let row_text: String = block.lines[0]
        .spans
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        !row_text.contains("ls"),
        "status row carries no command text"
    );
    assert!(!row_text.contains("→"), "status row carries no arrow");

    // Code row: highlighted command, no leading indent span (sits at lane col).
    let code_text: String = block.lines[1]
        .spans
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(code_text, "ls | select name type size");
    assert!(
        block.lines[1]
            .spans
            .iter()
            .any(|s| is_md_code_hint(&s.hint)),
        "code row must carry MdCode* hints, got: {:?}",
        block.lines[1]
            .spans
            .iter()
            .map(|s| s.hint.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        !block.lines[1]
            .spans
            .iter()
            .any(|s| matches!(s.hint, StyleHint::Muted)),
        "code row must not be Muted"
    );
}

#[test]
fn tool_invocation_nu_multi_line_command_renders_one_code_row_per_line() {
    // -- Setup & Fixtures
    let block = ToolInvocation {
        name: "nu".to_string(),
        source: "".to_string(),
        call_line: CallLineRender::CodeBlock {
            language: "nu".to_string(),
            code: "ls | where size > 1mb\n| select name type\n| sort-by modified".to_string(),
        },
    }
    .to_render_block();

    // -- Check
    assert_eq!(block.lines.len(), 4, "status row + 3 command lines");
    let row_text =
        |line: &ContentLine| -> String { line.spans.iter().map(|s| s.text.as_str()).collect() };
    assert_eq!(block.lines[0].spans.len(), 2, "name + source only");
    assert_eq!(row_text(&block.lines[1]), "ls | where size > 1mb");
    assert_eq!(row_text(&block.lines[2]), "| select name type");
    assert_eq!(row_text(&block.lines[3]), "| sort-by modified");
    for line in block.lines.iter().skip(1) {
        assert!(
            line.spans.iter().any(|s| is_md_code_hint(&s.hint)),
            "every code row must carry MdCode* hints"
        );
    }
}

#[test]
fn tool_invocation_nu_empty_args_renders_status_row_only() {
    // -- Setup & Fixtures
    let block = ToolInvocation {
        name: "nu".to_string(),
        source: "".to_string(),
        call_line: CallLineRender::CodeBlock {
            language: "nu".to_string(),
            code: String::new(),
        },
    }
    .to_render_block();

    // -- Check
    assert_eq!(block.lines.len(), 1, "status row only");
    assert_eq!(block.lines[0].spans.len(), 2, "name + source only");
}

#[test]
fn tool_invocation_non_nu_keeps_three_span_muted_rendering() {
    // -- Setup & Fixtures
    let block = ToolInvocation {
        name: "edit".to_string(),
        source: "builtin".to_string(),
        call_line: CallLineRender::Inline {
            summary: "→ {\"path\":\"a.rs\"}".to_string(),
        },
    }
    .to_render_block();

    // -- Check
    assert_eq!(block.lines.len(), 1);
    assert_eq!(block.lines[0].spans.len(), 3);
    assert_eq!(block.lines[0].spans[0], Span::emphasis("edit".to_string()));
    assert_eq!(block.lines[0].spans[1], Span::meta("builtin".to_string()));
    assert_eq!(
        block.lines[0].spans[2],
        Span::muted(" → {\"path\":\"a.rs\"}".to_string())
    );
}

#[test]
fn tool_result_empty_lines_uses_name_with_success_hint() {
    let block = ToolResult {
        name: "x".to_string(),
        success: true,
        lines: vec![],
    }
    .to_render_block();
    assert!(block.markdown.is_none());
    assert_eq!(block.lines.len(), 1);
    assert_eq!(block.lines[0].spans[0].text, "x");
    assert_eq!(block.lines[0].spans[0].hint, StyleHint::Success);
}

#[test]
fn tool_result_empty_lines_uses_name_with_error_hint() {
    let block = ToolResult {
        name: "x".to_string(),
        success: false,
        lines: vec![],
    }
    .to_render_block();
    assert_eq!(block.lines[0].spans[0].hint, StyleHint::Error);
}

#[test]
fn tool_result_maps_content_lines_to_render_block() {
    let block = ToolResult {
        name: "t".to_string(),
        success: true,
        lines: vec![
            ContentLine::single("+added".to_string(), StyleHint::DiffAdd),
            ContentLine::single("-removed".to_string(), StyleHint::DiffRemove),
        ],
    }
    .to_render_block();
    assert_eq!(block.lines.len(), 2);
    assert_eq!(block.lines[0].spans[0].text, "+added");
    assert_eq!(block.lines[0].spans[0].hint, StyleHint::DiffAdd);
    assert_eq!(block.lines[1].spans[0].text, "-removed");
    assert_eq!(block.lines[1].spans[0].hint, StyleHint::DiffRemove);
}

#[test]
fn compaction_notice_has_four_spans() {
    let block = CompactionNotice {
        source: "ctx".to_string(),
        summarized: 5,
        kept: 10,
        summary: "done".to_string(),
    }
    .to_render_block();
    assert_eq!(block.role, Role::Compaction);
    assert!(block.markdown.is_none());
    assert_eq!(block.lines[0].spans.len(), 4);
    assert_eq!(block.lines[0].spans[0], Span::meta("ctx".to_string()));
    assert_eq!(block.lines[0].spans[1], Span::normal("5".to_string()));
    assert_eq!(block.lines[0].spans[2], Span::normal("10".to_string()));
    assert_eq!(block.lines[0].spans[3], Span::normal("done".to_string()));
}

#[test]
fn transcript_entry_user_delegates_correctly() {
    let direct = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "z".to_string(),
        }),
        status: None,
    }
    .to_render_block();
    let via_enum = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "z".to_string(),
        }),
        status: None,
    }
    .to_render_block();
    assert_eq!(direct, via_enum);
}

#[test]
fn transcript_entry_role_returns_correct_role() {
    assert_eq!(
        TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::User(ProseMessage {
                markdown: "x".to_string(),
            }),
            status: None,
        }
        .role(),
        Role::User
    );
    assert_eq!(
        TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Tool(ToolInvocation {
                name: "t".to_string(),
                source: "".to_string(),
                call_line: CallLineRender::generic_json_summary(""),
            }),
            status: None,
        }
        .role(),
        Role::Tool
    );
}

#[test]
fn transcript_entry_text_returns_markdown_source() {
    assert_eq!(
        TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::User(ProseMessage {
                markdown: "hello world".to_string(),
            }),
            status: None,
        }
        .text(),
        "hello world"
    );
}

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
fn annotate_diff_hint_returns_normal_for_plain() {
    assert_eq!(annotate_diff_hint("plain"), StyleHint::Normal);
}

#[test]
fn user_and_assistant_render_blocks_differ_only_in_role() {
    let user_block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
    .to_render_block();
    let assistant_block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Assistant(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    }
    .to_render_block();
    assert_eq!(user_block.markdown, assistant_block.markdown);
    assert_eq!(user_block.role, Role::User);
    assert_eq!(assistant_block.role, Role::Assistant);
}

#[test]
fn user_message_text_accessor_returns_raw_markdown() {
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hello **world**\nagain".to_string(),
        }),
        status: None,
    };
    assert_eq!(entry.text(), "hello **world**\nagain");
}

#[test]
fn logo_entry_has_system_role_with_center_and_suppress_prefix() {
    let block = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Logo("test".to_string()),
        status: None,
    }
    .to_render_block();
    assert_eq!(block.role, Role::System);
    assert!(block.center, "logo must be centered");
    assert!(block.suppress_prefix, "logo must suppress lane prefix");
    assert!(block.markdown.is_none());
    assert!(!block.lines.is_empty());
}

#[test]
fn logo_entry_text_returns_raw_text() {
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::Logo("line1\nline2".to_string()),
        status: None,
    };
    assert_eq!(entry.text(), "line1\nline2");
}

#[test]
fn logo_entry_role_is_system() {
    assert_eq!(
        TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Logo("x".to_string()),
            status: None,
        }
        .role(),
        Role::System
    );
}

#[test]
fn transcript_entry_carries_status() {
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: Some(ItemStatus::Done),
    };
    assert_eq!(entry.status, Some(ItemStatus::Done));
}

#[test]
fn transcript_entry_defaults_to_none_status() {
    let entry = TranscriptEntry {
        id: 0,
        kind: TranscriptEntryKind::User(ProseMessage {
            markdown: "hi".to_string(),
        }),
        status: None,
    };
    assert!(entry.status.is_none());
}

// region:    --- Test Support

fn is_md_code_hint(hint: &StyleHint) -> bool {
    matches!(
        hint,
        StyleHint::MdCodeKeyword
            | StyleHint::MdCodeType
            | StyleHint::MdCodeFunction
            | StyleHint::MdCodeVariable
            | StyleHint::MdCodeConstant
            | StyleHint::MdCodeString
            | StyleHint::MdCodeNumber
            | StyleHint::MdCodeOperator
            | StyleHint::MdCodePunctuation
            | StyleHint::MdCodeComment
            | StyleHint::MdCodePlain
    )
}

// endregion: --- Test Support
