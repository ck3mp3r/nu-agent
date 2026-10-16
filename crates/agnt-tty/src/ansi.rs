use agnt_core::transcript::ir::StyleHint;

/// Map a `StyleHint` to ANSI escape codes, or return the input unchanged when
/// `use_color` is false (no ANSI output).
pub fn style_text(text: &str, hint: &StyleHint, use_color: bool) -> String {
    if !use_color {
        return text.to_string();
    }
    match hint {
        StyleHint::DiffAdd | StyleHint::Success => format!("\x1b[32m{text}\x1b[0m"),
        StyleHint::DiffRemove | StyleHint::Error => format!("\x1b[31m{text}\x1b[0m"),
        StyleHint::DiffHunk => format!("\x1b[1m{text}\x1b[0m"),
        StyleHint::MdBold => format!("\x1b[1;31m{text}\x1b[0m"),
        StyleHint::Meta | StyleHint::Muted => format!("\x1b[2m{text}\x1b[0m"),
        StyleHint::MdItalic => format!("\x1b[3;31m{text}\x1b[0m"),
        StyleHint::MdBoldItalic => format!("\x1b[1;3;31m{text}\x1b[0m"),
        StyleHint::MdInlineCode => format!("\x1b[32m{text}\x1b[0m"),
        StyleHint::MdCodeKeyword => format!("\x1b[35m{text}\x1b[0m"),
        StyleHint::MdCodeType => format!("\x1b[33m{text}\x1b[0m"),
        StyleHint::MdCodeFunction => format!("\x1b[34m{text}\x1b[0m"),
        StyleHint::MdCodeVariable => format!("\x1b[39m{text}\x1b[0m"),
        StyleHint::MdCodeConstant => format!("\x1b[33m{text}\x1b[0m"),
        StyleHint::MdCodeString => format!("\x1b[32m{text}\x1b[0m"),
        StyleHint::MdCodeNumber => format!("\x1b[33m{text}\x1b[0m"),
        StyleHint::MdCodeOperator => format!("\x1b[36m{text}\x1b[0m"),
        StyleHint::MdCodePunctuation => format!("\x1b[90m{text}\x1b[0m"),
        StyleHint::MdCodeComment => format!("\x1b[3;90m{text}\x1b[0m"),
        StyleHint::MdHeading(1) => format!("\x1b[1;31m{text}\x1b[0m"),
        StyleHint::MdHeading(2) | StyleHint::MdHeading(3) => {
            format!("\x1b[1;33m{text}\x1b[0m")
        }
        StyleHint::MdHeading(4) => format!("\x1b[1;32m{text}\x1b[0m"),
        StyleHint::MdHeading(5) => format!("\x1b[1;36m{text}\x1b[0m"),
        StyleHint::MdHeading(6) => format!("\x1b[1;95m{text}\x1b[0m"),
        StyleHint::MdHeading(_) => format!("\x1b[1;34m{text}\x1b[0m"),
        StyleHint::MdLink => format!("\x1b[95m{text}\x1b[0m"),
        StyleHint::MdLinkUrl => format!("\x1b[3;4;34m{text}\x1b[0m"),
        StyleHint::MdQuote => format!("\x1b[35m{text}\x1b[0m"),
        StyleHint::MdListMarker => format!("\x1b[36m{text}\x1b[0m"),
        StyleHint::Normal | StyleHint::Emphasis | StyleHint::Cancelled | StyleHint::MdCodePlain => {
            text.to_string()
        }
    }
}

#[cfg(test)]
#[path = "../test/ansi.rs"]
mod ansi_test;
