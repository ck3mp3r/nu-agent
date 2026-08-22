use nu_agent_core::transcript::ir::StyleHint;

/// Map a `StyleHint` to ANSI escape codes, or return the input unchanged when
/// `use_color` is false (no ANSI output).
pub fn style_text(text: &str, hint: &StyleHint, use_color: bool) -> String {
    if !use_color {
        return text.to_string();
    }
    match hint {
        StyleHint::DiffAdd | StyleHint::Success => format!("\x1b[32m{text}\x1b[0m"),
        StyleHint::DiffRemove | StyleHint::Error => format!("\x1b[31m{text}\x1b[0m"),
        StyleHint::DiffHunk | StyleHint::MdBold => format!("\x1b[1m{text}\x1b[0m"),
        StyleHint::Meta | StyleHint::Muted => format!("\x1b[2m{text}\x1b[0m"),
        StyleHint::MdItalic => format!("\x1b[3m{text}\x1b[0m"),
        StyleHint::MdBoldItalic => format!("\x1b[1;3m{text}\x1b[0m"),
        StyleHint::MdInlineCode => format!("\x1b[2;33m{text}\x1b[0m"),
        StyleHint::MdCodeKeyword => format!("\x1b[35m{text}\x1b[0m"),
        StyleHint::MdCodeType => format!("\x1b[33m{text}\x1b[0m"),
        StyleHint::MdCodeFunction => format!("\x1b[34m{text}\x1b[0m"),
        StyleHint::MdCodeVariable => format!("\x1b[36m{text}\x1b[0m"),
        StyleHint::MdCodeConstant => format!("\x1b[31m{text}\x1b[0m"),
        StyleHint::MdCodeString => format!("\x1b[32m{text}\x1b[0m"),
        StyleHint::MdCodeNumber => format!("\x1b[33m{text}\x1b[0m"),
        StyleHint::MdCodeOperator => format!("\x1b[36m{text}\x1b[0m"),
        StyleHint::MdCodePunctuation => format!("\x1b[2m{text}\x1b[0m"),
        StyleHint::MdCodeComment => format!("\x1b[2;90m{text}\x1b[0m"),
        StyleHint::Normal | StyleHint::Emphasis | StyleHint::Cancelled | StyleHint::MdCodePlain => {
            text.to_string()
        }
    }
}

#[cfg(test)]
#[path = "ansi_test.rs"]
mod ansi_test;
