fn strip_pseudo_code_tags(markdown: &str) -> String {
    let mut remaining = markdown;
    let mut sanitized = String::with_capacity(markdown.len());

    while let Some(start) = remaining.find("[code:") {
        sanitized.push_str(&remaining[..start]);
        let after_start = &remaining[start..];
        if let Some(end) = after_start.find(']') {
            remaining = &after_start[end + 1..];
        } else {
            remaining = "";
            break;
        }
    }

    sanitized.push_str(remaining);
    sanitized.replace("[/code]", "")
}

fn strip_known_control_blocks(markdown: &str) -> String {
    let start_tag = "<system-reminder>";
    let end_tag = "</system-reminder>";

    let mut sanitized = markdown.to_string();
    while let Some(start) = sanitized.find(start_tag) {
        let after_start = start + start_tag.len();
        if let Some(end_rel) = sanitized[after_start..].find(end_tag) {
            let end = after_start + end_rel + end_tag.len();
            sanitized.replace_range(start..end, "");
        } else {
            sanitized.replace_range(start.., "");
            break;
        }
    }

    sanitized
}

pub(super) fn sanitize_assistant_visible_markdown(markdown: &str) -> String {
    let without_control_blocks = strip_known_control_blocks(markdown);
    strip_pseudo_code_tags(&without_control_blocks)
}
