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
            return String::new();
        }

        // 1. Role prefix
        let prefix = match block.role {
            Role::User => "[user] ",
            Role::Assistant => "",
            Role::Tool => "[tool] ",
            Role::ToolDisplay => "  ",
            Role::Compaction => "[compaction] ",
            Role::System => "[system] ",
            Role::Separator => "", // handled above; kept for exhaustiveness
        };

        // 2. Status indicator
        let indicator = match ctx.status {
            Some(ItemStatus::Done) => "✓ ",
            Some(ItemStatus::Failed) => "✕ ",
            Some(ItemStatus::InProgress) => "… ",
            Some(ItemStatus::Queued) => "• ",
            Some(ItemStatus::Cancelled) => "✕ ",
            Some(ItemStatus::Unknown) => "? ",
            None => "",
        };

        // 3. Content: when markdown is present, use the raw text directly (TTY
        // renders plain text; rich projection is the TUI's job). When pre-built
        // ContentLine values are provided, use those instead.
        let mut content_parts = Vec::new();

        if let Some(md) = &block.markdown {
            // Split raw markdown by line and join; strip blank lines so the
            // output looks clean on a plain terminal.
            for line in md.lines() {
                if !line.trim().is_empty() {
                    content_parts.push(line.to_string());
                }
            }
        } else {
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
        }

        let content = content_parts.join("\n");

        // 4. Combine all parts
        format!("{prefix}{indicator}{content}")
    }
}

impl TtyRenderer {
    fn colorize(&self, text: &str, hint: &StyleHint) -> String {
        crate::ansi::style_text(text, hint, self.use_color)
    }
}
