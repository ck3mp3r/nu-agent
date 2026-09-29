use nu_agent_core::transcript::ir::{
    Block, BlockSource, ContentKind, ContentLine, Lane, StyleHint,
};
use nu_agent_core::transcript::items::annotate_diff_hint;
use nu_agent_core::transcript::renderer::FrameContext;

use crate::ansi::style_text;

/// The pure-function TTY renderer: projects a flat Block to text with ANSI
/// codes. Same (Block, FrameContext) → output contract as the TUI's layout —
/// same inputs, same output, no renderer instance state. TTY-specific
/// projection: the lane renders as a text prefix ([user], [tool], [system]),
/// the status renders as a one-character indicator, and Fill is ignored
/// (terminal text has no background fills).
///
/// `use_color` gates ANSI output: `true` emits escape codes for styled spans,
/// `false` returns plain text. Callers pass the terminal's color capability
/// (the streaming path gates on stderr-is-tty).
pub fn layout(block: &Block, ctx: &FrameContext, use_color: bool) -> String {
    // Fill is a TUI concept; the TTY ignores it.
    let _ = block.fill;
    let _ = ctx.width;

    if matches!(block.source, BlockSource::Spacer) {
        return String::new();
    }

    // 1. Lane prefix (row 0 only; single-block rendering has no continuations).
    // A ToolDisplay block carries `Lane::Blank` (its content is pre-projected
    // tool output, not assistant prose) but keeps the pre-refactor 2-space
    // indent prefix (`Role::ToolDisplay => "  "` in the old renderer), so the
    // prefix distinguishes it from assistant prose by source kind.
    let prefix = if matches!(block.source, BlockSource::ToolDisplay { .. }) {
        "  "
    } else {
        match block.lane {
            Lane::Marker(icon) => lane_text_prefix(icon),
            Lane::Blank | Lane::SystemBlank => "",
        }
    };

    // 2. Status indicator — the shared `ItemStatus::indicator_char` source
    // (the TUI uses the same method), plus the trailing space the format
    // string below requires. `InProgress` therefore animates in the TTY just
    // as it does in the TUI (task 6ca8b0ad).
    let indicator = match block.status {
        Some(status) => format!("{} ", status.indicator_char(ctx.now_millis)),
        None => String::new(),
    };

    // 3. Content projection, joined with newlines. Empty content keeps the
    // row free of a dangling prefix+indicator.
    let content = render_content(block, use_color);
    if content.is_empty() {
        String::new()
    } else {
        format!("{prefix}{indicator}{content}")
    }
}

// region:    --- Support

/// Map a lane marker glyph to its text prefix. Empty for glyphs with no TTY
/// text equivalent (assistant, banner, spacer).
fn lane_text_prefix(icon: &str) -> &'static str {
    match icon {
        "▏" => "[user] ",
        "⚙" => "[tool] ",
        "~" => "[compaction] ",
        "·" => "[system] ",
        _ => "",
    }
}

/// Project the block's source into styled text, joined with newlines. Diff
/// sections are annotated through `annotate_diff_hint` (hunk bold, file-meta
/// dim, add green, remove red); code and plain sections render verbatim.
///
/// `use_color` is threaded to every `style_text` call so `false` yields plain
/// text with no escape codes. Markdown content drops blank lines (the old
/// TTY filtered them so plain terminals show no spurious gaps).
fn render_content(block: &Block, use_color: bool) -> String {
    match &block.source {
        BlockSource::Markdown { markdown, .. } => markdown
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        BlockSource::Tool {
            name,
            call,
            preview,
        } => {
            // Row 0 carries the tool name (emphasis — plain text in the TTY
            // palette) followed by the call summary, mirroring the pre-Block
            // model's span join. An empty summary renders the name alone with
            // no trailing space. A nameless block renders the summary alone.
            let name_text = name.0.as_str();
            let call_line = if name_text.is_empty() {
                call.summary.clone()
            } else if call.summary.is_empty() {
                style_text(name_text, &StyleHint::Emphasis, use_color)
            } else {
                format!(
                    "{} {}",
                    style_text(name_text, &StyleHint::Emphasis, use_color),
                    call.summary
                )
            };
            let mut parts = vec![call_line];
            if let Some(display) = preview {
                for section in &display.sections {
                    match &section.kind {
                        ContentKind::Diff { .. } => {
                            for line in section.content.lines() {
                                parts.push(style_text(line, &annotate_diff_hint(line), use_color));
                            }
                        }
                        ContentKind::Code { .. } => {
                            for line in section.content.lines() {
                                parts.push(style_text(line, &StyleHint::Normal, use_color));
                            }
                        }
                        ContentKind::Plain => {
                            parts.push(style_text(&section.content, &StyleHint::Normal, use_color));
                        }
                    }
                    append_stats(&mut parts, section.stats.as_ref(), use_color);
                }
            }
            parts.join("\n")
        }
        BlockSource::Notice { text, .. } | BlockSource::Banner { text } => text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        BlockSource::ToolDisplay { lines } => lines
            .iter()
            .map(|line| render_content_line(line, use_color))
            .collect::<Vec<_>>()
            .join("\n"),
        BlockSource::Spacer => String::new(),
    }
}

/// Render one pre-projected ContentLine: concatenate spans with per-span ANSI
/// styling gated by `use_color`.
fn render_content_line(line: &ContentLine, use_color: bool) -> String {
    line.spans
        .iter()
        .map(|span| style_text(&span.text, &span.hint, use_color))
        .collect()
}

/// Append a section's stats line ("N files changed, M insertions, ...")
/// styled as muted, mirroring the streaming renderer's stats formatting.
fn append_stats(
    parts: &mut Vec<String>,
    stats: Option<&nu_agent_core::transcript::ir::DisplayStats>,
    use_color: bool,
) {
    let Some(stats) = stats else { return };
    let mut fields = Vec::new();
    if let Some(f) = stats.files_changed {
        fields.push(format!("{f} files changed"));
    }
    if let Some(i) = stats.insertions {
        fields.push(format!("{i} insertions"));
    }
    if let Some(d) = stats.deletions {
        fields.push(format!("{d} deletions"));
    }
    if let Some(o) = stats.omitted_files {
        fields.push(format!("{o} files omitted"));
    }
    if let Some(o) = stats.omitted_hunks {
        fields.push(format!("{o} hunks omitted"));
    }
    if !fields.is_empty() {
        parts.push(style_text(&fields.join(", "), &StyleHint::Muted, use_color));
    }
}

// endregion: --- Support
