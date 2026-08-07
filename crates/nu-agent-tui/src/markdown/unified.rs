use nu_agent_core::transcript::ir::ContentLine;

use crate::markdown::project_markdown_to_lines;

/// Project raw markdown text into IR ContentLines using the existing
/// pulldown-cmark pipeline. Preserves single blank lines between blocks
/// for readability, but collapses consecutive blanks and drops leading/trailing ones.
///
/// `max_width` is threaded through to the projection layer for future width-aware
/// table rendering; it is not yet used for clamping (that is a separate task).
pub fn render_markdown_lines(text: &str, max_width: Option<u16>) -> Vec<ContentLine> {
    let projected: Vec<ContentLine> = project_markdown_to_lines(text, max_width);

    // Collapse consecutive blank lines to at most one, strip leading/trailing blanks
    let mut result = Vec::with_capacity(projected.len());
    let mut prev_blank = false;
    for line in projected {
        let is_blank = line.spans.is_empty();
        if is_blank {
            if !prev_blank && !result.is_empty() {
                result.push(line);
            }
            prev_blank = true;
        } else {
            result.push(line);
            prev_blank = false;
        }
    }
    // Trim trailing blank
    if result.last().is_some_and(|l| l.spans.is_empty()) {
        result.pop();
    }
    result
}
