use nu_agent_core::transcript::ir::*;
use nu_agent_core::transcript::items::*;
use nu_agent_core::transcript::renderer::*;

use crate::tty_renderer::TtyRenderer;

fn plain() -> TtyRenderer {
    TtyRenderer { use_color: false }
}

fn colored() -> TtyRenderer {
    TtyRenderer { use_color: true }
}

fn ctx() -> RenderContext {
    RenderContext {
        width: 80,
        cursor: false,
        selected: false,
        status: None,
        now_millis: 0,
    }
}

#[test]
fn user_message_has_user_prefix() {
    let r = plain();
    let block = TranscriptEntry::User(ProseMessage {
        markdown: "hello".to_string(),
    })
    .to_render_block();
    let out = r.render(&block, &ctx());
    assert!(out.starts_with("[user] "), "got: {out}");
    assert!(out.contains("hello"));
}

#[test]
fn assistant_has_no_prefix() {
    let r = plain();
    let block = TranscriptEntry::Assistant(ProseMessage {
        markdown: "hi".to_string(),
    })
    .to_render_block();
    let out = r.render(&block, &ctx());
    assert!(
        !out.contains("[assistant]"),
        "should have no prefix, got: {out}"
    );
    assert!(
        out.starts_with("hi") || out.starts_with("hi"),
        "should start with content"
    );
}

#[test]
fn tool_shows_tool_prefix() {
    let r = plain();
    let block = ToolInvocation {
        name: "run".to_string(),
        source: "".to_string(),
        args: "{}".to_string(),
    }
    .to_render_block();
    let out = r.render(&block, &ctx());
    assert!(out.starts_with("[tool] "), "got: {out}");
}

#[test]
fn separator_renders_empty_string() {
    let r = plain();
    let block = Spacer.to_render_block();
    let out = r.render(&block, &ctx());
    assert_eq!(out, "");
}

#[test]
fn done_status_shows_checkmark() {
    let r = plain();
    let block = ToolInvocation {
        name: "t".to_string(),
        source: "".to_string(),
        args: "".to_string(),
    }
    .to_render_block();
    let mut c = ctx();
    c.status = Some(ItemStatus::Done);
    let out = r.render(&block, &c);
    assert!(out.contains("✓"), "got: {out}");
}

#[test]
fn no_ansi_when_color_false() {
    let r = plain();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "+added".to_string(),
            StyleHint::DiffAdd,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let out = r.render(&block, &ctx());
    assert!(
        !out.contains("\x1b"),
        "should have no ANSI codes, got: {out}"
    );
}

#[test]
fn ansi_green_for_diff_add_when_color_true() {
    let r = colored();
    let block = RenderBlock {
        role: Role::ToolDisplay,
        lines: vec![ContentLine::single(
            "+added".to_string(),
            StyleHint::DiffAdd,
        )],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let out = r.render(&block, &ctx());
    assert!(
        out.contains("\x1b[32m"),
        "should have green ANSI, got: {out}"
    );
    assert!(out.contains("+added"));
}

#[test]
fn multi_line_separated_by_newlines() {
    let r = plain();
    let block = RenderBlock {
        role: Role::System,
        lines: vec![
            ContentLine::single("line1".to_string(), StyleHint::Normal),
            ContentLine::single("line2".to_string(), StyleHint::Normal),
        ],
        markdown: None,
        center: false,
        suppress_prefix: false,
    };
    let out = r.render(&block, &ctx());
    assert!(
        out.contains("line1\nline2"),
        "should have newline between lines, got: {out}"
    );
}
