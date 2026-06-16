use nu_agent_core::transcript::ir::{RenderBlock, Role, StyleHint};
use nu_agent_core::transcript::renderer::{BlockRenderer, ItemStatus, RenderContext};

pub struct TtyRenderer {
    pub use_color: bool,
}

impl BlockRenderer for TtyRenderer {
    type Output = String;

    fn render(&self, block: &RenderBlock, ctx: &RenderContext) -> String {
        // Handle separator special case first
        if block.role == Role::Separator {
            return "---".to_string();
        }

        // 1. Role prefix
        let prefix = match block.role {
            Role::User => "[user] ",
            Role::Assistant => "",
            Role::Tool => "[tool] ",
            Role::ToolDisplay => "  ",
            Role::Compaction => "[compaction] ",
            Role::System => "[system] ",
            Role::Separator => unreachable!(), // handled above
        };

        // 2. Status indicator
        let indicator = match ctx.status {
            Some(ItemStatus::Done) => "✓ ",
            Some(ItemStatus::Failed) => "✕ ",
            Some(ItemStatus::InProgress) => "… ",
            Some(ItemStatus::Queued) => "• ",
            Some(ItemStatus::Cancelled) => "✕ ",
            None => "",
        };

        // 3. Content - concatenate all spans from all lines
        let mut content_parts = Vec::new();

        for line in &block.lines {
            let mut line_text = String::new();
            for span in &line.spans {
                let text = if self.use_color {
                    self.colorize(&span.text, &span.hint)
                } else {
                    span.text.clone()
                };
                line_text.push_str(&text);
            }
            content_parts.push(line_text);
        }

        let content = content_parts.join("\n");

        // 4. Combine all parts
        format!("{prefix}{indicator}{content}")
    }
}

impl TtyRenderer {
    fn colorize(&self, text: &str, hint: &StyleHint) -> String {
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
            StyleHint::Normal
            | StyleHint::Emphasis
            | StyleHint::Cancelled
            | StyleHint::MdCodePlain => text.to_string(),
        }
    }
}
