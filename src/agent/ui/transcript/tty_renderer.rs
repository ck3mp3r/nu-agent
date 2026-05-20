use super::ir::{RenderBlock, Role, StyleHint};
use super::renderer::{BlockRenderer, ItemStatus, RenderContext};

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
            StyleHint::DiffAdd | StyleHint::Success => format!("\x1b[32m{}\x1b[0m", text),
            StyleHint::DiffRemove | StyleHint::Error => format!("\x1b[31m{}\x1b[0m", text),
            StyleHint::DiffHunk => format!("\x1b[1m{}\x1b[0m", text),
            StyleHint::Meta | StyleHint::Muted => format!("\x1b[2m{}\x1b[0m", text),
            StyleHint::Normal | StyleHint::Emphasis | StyleHint::Cancelled => text.to_string(),
            StyleHint::Rendered(style) => {
                // Map ratatui Style to ANSI — extract fg color if present
                if let Some(color) = style.fg {
                    match color {
                        ratatui::style::Color::Green => format!("\x1b[32m{}\x1b[0m", text),
                        ratatui::style::Color::Red => format!("\x1b[31m{}\x1b[0m", text),
                        ratatui::style::Color::Yellow => format!("\x1b[33m{}\x1b[0m", text),
                        ratatui::style::Color::Blue => format!("\x1b[34m{}\x1b[0m", text),
                        ratatui::style::Color::Magenta => format!("\x1b[35m{}\x1b[0m", text),
                        ratatui::style::Color::Cyan => format!("\x1b[36m{}\x1b[0m", text),
                        _ => {
                            if style.add_modifier.contains(ratatui::style::Modifier::BOLD) {
                                format!("\x1b[1m{}\x1b[0m", text)
                            } else {
                                text.to_string()
                            }
                        }
                    }
                } else if style.add_modifier.contains(ratatui::style::Modifier::BOLD) {
                    format!("\x1b[1m{}\x1b[0m", text)
                } else if style
                    .add_modifier
                    .contains(ratatui::style::Modifier::ITALIC)
                {
                    format!("\x1b[3m{}\x1b[0m", text)
                } else {
                    text.to_string()
                }
            }
        }
    }
}
