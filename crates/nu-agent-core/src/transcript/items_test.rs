use super::ir::*;
use super::items::*;

#[test]
fn user_message_produces_user_role_block() {
    let block = TranscriptEntry::User(ProseMessage {
        lines: vec![ContentLine::single("hi".to_string(), StyleHint::Normal)],
    })
    .to_render_block();
    assert_eq!(block.role, Role::User);
    assert_eq!(block.lines.len(), 1);
    assert_eq!(block.lines[0].spans.len(), 1);
    assert_eq!(block.lines[0].spans[0].text, "hi");
    assert_eq!(block.lines[0].spans[0].hint, StyleHint::Normal);
}

#[test]
fn assistant_chunk_produces_assistant_role_block() {
    let block = TranscriptEntry::Assistant(ProseMessage {
        lines: vec![ContentLine::single("hello".to_string(), StyleHint::Normal)],
    })
    .to_render_block();
    assert_eq!(block.role, Role::Assistant);
    assert_eq!(block.lines.len(), 1);
    assert_eq!(block.lines[0].spans[0].text, "hello");
    assert_eq!(block.lines[0].spans[0].hint, StyleHint::Normal);
}

#[test]
fn tool_invocation_produces_three_spans() {
    let block = ToolInvocation {
        name: "nu__run".to_string(),
        source: "builtin".to_string(),
        args: "{\"cmd\":\"ls\"}".to_string(),
    }
    .to_render_block();
    assert_eq!(block.role, Role::Tool);
    assert_eq!(block.lines[0].spans.len(), 3);
    assert_eq!(
        block.lines[0].spans[0],
        Span::emphasis("nu__run".to_string())
    );
    assert_eq!(block.lines[0].spans[1], Span::meta("builtin".to_string()));
    assert_eq!(
        block.lines[0].spans[2],
        Span::muted(" {\"cmd\":\"ls\"}".to_string())
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
fn tool_result_maps_display_lines_to_content_lines() {
    let block = ToolResult {
        name: "t".to_string(),
        success: true,
        lines: vec![
            DisplayLine::new("+added".to_string(), StyleHint::DiffAdd),
            DisplayLine::new("-removed".to_string(), StyleHint::DiffRemove),
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
    assert_eq!(block.lines[0].spans.len(), 4);
    assert_eq!(block.lines[0].spans[0], Span::meta("ctx".to_string()));
    assert_eq!(block.lines[0].spans[1], Span::normal("5".to_string()));
    assert_eq!(block.lines[0].spans[2], Span::normal("10".to_string()));
    assert_eq!(block.lines[0].spans[3], Span::normal("done".to_string()));
}

#[test]
fn separator_has_empty_lines_vec() {
    let block = Separator.to_render_block();
    assert_eq!(block.role, Role::Separator);
    assert!(block.lines.is_empty());
}

#[test]
fn transcript_entry_user_delegates_correctly() {
    let direct = TranscriptEntry::User(ProseMessage {
        lines: vec![ContentLine::single("z".to_string(), StyleHint::Normal)],
    })
    .to_render_block();
    let via_enum = TranscriptEntry::User(ProseMessage {
        lines: vec![ContentLine::single("z".to_string(), StyleHint::Normal)],
    })
    .to_render_block();
    assert_eq!(direct, via_enum);
}

#[test]
fn transcript_entry_role_returns_correct_role() {
    assert_eq!(
        TranscriptEntry::User(ProseMessage {
            lines: vec![ContentLine::single("x".to_string(), StyleHint::Normal)],
        })
        .role(),
        Role::User
    );
    assert_eq!(
        TranscriptEntry::Tool(ToolInvocation {
            name: "t".to_string(),
            source: "".to_string(),
            args: "".to_string(),
        })
        .role(),
        Role::Tool
    );
    assert_eq!(
        TranscriptEntry::Separator(Separator).role(),
        Role::Separator
    );
}

#[test]
fn transcript_entry_text_returns_content() {
    assert_eq!(
        TranscriptEntry::User(ProseMessage {
            lines: vec![ContentLine::single("hello".to_string(), StyleHint::Normal)],
        })
        .text(),
        "hello"
    );
    assert_eq!(
        TranscriptEntry::Separator(Separator).text(),
        "────────────────"
    );
}

#[test]
fn parse_tool_text_extracts_name_and_args() {
    let (name, args) = parse_tool_text("tool[nu__run] args={}");
    assert_eq!(name, "nu__run");
    assert_eq!(args, "args={}");
}

#[test]
fn parse_tool_text_handles_no_prefix() {
    let (name, args) = parse_tool_text("plain text");
    assert_eq!(name, "plain text");
    assert_eq!(args, "");
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
    let lines = vec![ContentLine::single("hi".to_string(), StyleHint::MdBold)];
    let user_block = TranscriptEntry::User(ProseMessage {
        lines: lines.clone(),
    })
    .to_render_block();
    let assistant_block = TranscriptEntry::Assistant(ProseMessage {
        lines: lines.clone(),
    })
    .to_render_block();
    assert_eq!(user_block.lines, assistant_block.lines);
    assert_eq!(user_block.role, Role::User);
    assert_eq!(assistant_block.role, Role::Assistant);
}

#[test]
fn user_message_text_accessor_concatenates_spans() {
    let entry = TranscriptEntry::User(ProseMessage {
        lines: vec![
            ContentLine::from_spans(vec![
                Span::new("hello ".to_string(), StyleHint::Normal),
                Span::new("world".to_string(), StyleHint::MdBold),
            ]),
            ContentLine::single("again".to_string(), StyleHint::Normal),
        ],
    });
    assert_eq!(entry.text(), "hello world\nagain");
}
