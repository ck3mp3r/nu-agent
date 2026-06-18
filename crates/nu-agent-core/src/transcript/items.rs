use super::ir::{ContentLine, DisplayLine, RenderBlock, Role, Span, StyleHint};

pub trait Renderable {
    fn to_render_block(&self) -> RenderBlock;
}

/// Markdown-projected prose authored by the user or assistant.
/// The role (and therefore lane prefix/background) is determined by which
/// `TranscriptEntry` variant wraps this value, not by the message itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseMessage {
    pub lines: Vec<ContentLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub name: String,
    pub source: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub name: String,
    pub success: bool,
    pub lines: Vec<DisplayLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionNotice {
    pub source: String,
    pub summarized: usize,
    pub kept: usize,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemMessage {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Separator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spacer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEntry {
    User(ProseMessage),
    Assistant(ProseMessage),
    Tool(ToolInvocation),
    ToolResult(ToolResult),
    Compaction(CompactionNotice),
    System(SystemMessage),
    Separator(Separator),
    Spacer(Spacer),
}

impl Renderable for ToolInvocation {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::Tool,
            lines: vec![ContentLine::from_spans(vec![
                Span::emphasis(self.name.clone()),
                Span::meta(self.source.clone()),
                Span::muted(format!(" {}", self.args)),
            ])],
        }
    }
}

impl Renderable for ToolResult {
    fn to_render_block(&self) -> RenderBlock {
        let lines = if self.lines.is_empty() {
            vec![ContentLine::single(
                self.name.clone(),
                if self.success {
                    StyleHint::Success
                } else {
                    StyleHint::Error
                },
            )]
        } else {
            self.lines
                .iter()
                .map(|dl| ContentLine::single(dl.text.clone(), dl.hint.clone()))
                .collect()
        };
        RenderBlock {
            role: Role::ToolDisplay,
            lines,
        }
    }
}

impl Renderable for CompactionNotice {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::Compaction,
            lines: vec![ContentLine::from_spans(vec![
                Span::meta(self.source.clone()),
                Span::normal(self.summarized.to_string()),
                Span::normal(self.kept.to_string()),
                Span::normal(self.summary.clone()),
            ])],
        }
    }
}

impl Renderable for SystemMessage {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::System,
            lines: vec![ContentLine::single(self.text.clone(), StyleHint::Normal)],
        }
    }
}

impl Renderable for Separator {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::Separator,
            lines: vec![],
        }
    }
}

impl Renderable for Spacer {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::Separator,
            lines: vec![ContentLine::single(String::new(), StyleHint::Normal)],
        }
    }
}

impl Renderable for TranscriptEntry {
    fn to_render_block(&self) -> RenderBlock {
        match self {
            Self::User(m) => RenderBlock {
                role: Role::User,
                lines: m.lines.clone(),
            },
            Self::Assistant(m) => RenderBlock {
                role: Role::Assistant,
                lines: m.lines.clone(),
            },
            Self::Tool(t) => t.to_render_block(),
            Self::ToolResult(r) => r.to_render_block(),
            Self::Compaction(c) => c.to_render_block(),
            Self::System(s) => s.to_render_block(),
            Self::Separator(s) => s.to_render_block(),
            Self::Spacer(s) => s.to_render_block(),
        }
    }
}

impl TranscriptEntry {
    pub fn role(&self) -> Role {
        match self {
            Self::User(_) => Role::User,
            Self::Assistant(_) => Role::Assistant,
            Self::Tool(_) => Role::Tool,
            Self::ToolResult(_) => Role::ToolDisplay,
            Self::Compaction(_) => Role::Compaction,
            Self::System(_) => Role::System,
            Self::Separator(_) => Role::Separator,
            Self::Spacer(_) => Role::Separator,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::User(m) | Self::Assistant(m) => m
                .lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Tool(t) => t.name.clone(),
            Self::ToolResult(r) => r.name.clone(),
            Self::Compaction(c) => c.summary.clone(),
            Self::System(s) => s.text.clone(),
            Self::Separator(_) => "────────────────".to_string(),
            Self::Spacer(_) => String::new(),
        }
    }
}

/// Parse "tool[name] args=..." format into (name, args)
pub fn parse_tool_text(text: &str) -> (String, String) {
    if let Some(rest) = text.strip_prefix("tool[")
        && let Some((name, tail)) = rest.split_once(']')
    {
        return (name.to_string(), tail.trim().to_string());
    }
    (text.to_string(), String::new())
}

/// Annotate a line of text with diff-style StyleHint
pub fn annotate_diff_hint(text: &str) -> StyleHint {
    let trimmed = text.trim_start();
    if trimmed.starts_with("@@ ") {
        return StyleHint::DiffHunk;
    }
    if trimmed.starts_with("--- ") || trimmed.starts_with("+++ ") {
        return StyleHint::Meta;
    }
    if trimmed.starts_with('+') {
        return StyleHint::DiffAdd;
    }
    if trimmed.starts_with('-') {
        return StyleHint::DiffRemove;
    }
    if trimmed.starts_with("\\ ") {
        return StyleHint::Meta;
    }
    StyleHint::Normal
}
