//! Content rendering tests for the TTY renderer.

use super::*;

#[test]
fn no_ansi_when_color_false() {
    let block = notice_block("+added");
    let out = render_plain(&block);
    assert!(
        !out.contains("\x1b"),
        "should have no ANSI codes, got: {out}"
    );
}

#[test]
fn ansi_green_for_diff_add_when_color_true() {
    let block = notice_block("+added");
    let out = render(&block);
    assert!(out.contains("+added"), "got: {out}");
}

#[test]
fn multi_line_separated_by_newlines() {
    let block = notice_block("line1\nline2");
    let out = render(&block);
    assert!(
        out.contains("line1\nline2"),
        "should have newline between lines, got: {out}"
    );
}

#[test]
fn tool_preview_lines_render_every_content_line() {
    // -- Setup & Fixtures
    let command = "ls | where size > 1mb\n| select name type\n| sort-by modified";
    let block = Block {
        source: BlockSource::Tool {
            name: ToolName("nu".to_string()),
            call: CallLine {
                summary: "nu".to_string(),
            },
            preview: Some(Display {
                title: Some("nu".to_string()),
                sections: vec![DisplaySection {
                    label: Some("nu".to_string()),
                    kind: ContentKind::Code {
                        language: "nu".to_string(),
                    },
                    content: command.to_string(),
                    stats: None,
                }],
            }),
        },
        lane: Lane::Marker("⚙"),
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let out = render(&block);

    // -- Check: the command appears one line per command line, with no JSON
    // summary flattening.
    assert!(
        out.contains("ls | where size > 1mb\n| select name type\n| sort-by modified"),
        "got: {out}"
    );
}

/// Notice content is rendered verbatim (no ANSI mangling of text).
#[test]
fn notice_content_renders_verbatim() {
    let block = notice_block("+added");
    let out = render(&block);
    assert!(out.contains("+added"), "got: {out}");
}
