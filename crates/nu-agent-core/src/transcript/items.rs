use super::ir::{ContentLine, DisplayLine, RenderBlock, Role, Span, StyleHint};
use super::renderer::ItemStatus;

pub trait Renderable {
    fn to_render_block(&self) -> RenderBlock;
}

/// Markdown-projected prose authored by the user or assistant.
/// The role (and therefore lane prefix/background) is determined by which
/// `TranscriptEntry` variant wraps this value, not by the message itself.
///
/// Stores raw markdown source. Projection to `ContentLine` happens at render
/// time so the available canvas width can be taken into account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseMessage {
    pub markdown: String,
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
pub struct Spacer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEntryKind {
    User(ProseMessage),
    Assistant(ProseMessage),
    Tool(ToolInvocation),
    ToolResult(ToolResult),
    Compaction(CompactionNotice),
    System(SystemMessage),
    Spacer(Spacer),
    Logo(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub id: u64,
    pub kind: TranscriptEntryKind,
    pub status: Option<ItemStatus>,
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
            markdown: None,
            center: false,
            suppress_prefix: false,
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
            markdown: None,
            center: false,
            suppress_prefix: false,
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
            markdown: None,
            center: false,
            suppress_prefix: false,
        }
    }
}

impl Renderable for SystemMessage {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::System,
            lines: vec![ContentLine::single(self.text.clone(), StyleHint::Normal)],
            markdown: None,
            center: false,
            suppress_prefix: false,
        }
    }
}
impl Renderable for Spacer {
    fn to_render_block(&self) -> RenderBlock {
        RenderBlock {
            role: Role::Separator,
            lines: vec![ContentLine::single(String::new(), StyleHint::Normal)],
            markdown: None,
            center: false,
            suppress_prefix: false,
        }
    }
}

impl Renderable for TranscriptEntryKind {
    fn to_render_block(&self) -> RenderBlock {
        match self {
            Self::User(m) => RenderBlock {
                role: Role::User,
                lines: vec![],
                markdown: Some(m.markdown.clone()),
                center: false,
                suppress_prefix: false,
            },
            Self::Assistant(m) => RenderBlock {
                role: Role::Assistant,
                lines: vec![],
                markdown: Some(m.markdown.clone()),
                center: false,
                suppress_prefix: false,
            },
            Self::Tool(t) => t.to_render_block(),
            Self::ToolResult(r) => r.to_render_block(),
            Self::Compaction(c) => c.to_render_block(),
            Self::System(s) => s.to_render_block(),
            Self::Spacer(s) => s.to_render_block(),
            Self::Logo(text) => RenderBlock {
                role: Role::System,
                lines: text
                    .lines()
                    .map(|line| ContentLine::single(line.to_string(), StyleHint::Normal))
                    .collect(),
                markdown: None,
                center: true,
                suppress_prefix: true,
            },
        }
    }
}

impl Renderable for TranscriptEntry {
    fn to_render_block(&self) -> RenderBlock {
        self.kind.to_render_block()
    }
}

impl TranscriptEntryKind {
    pub fn role(&self) -> Role {
        match self {
            Self::User(_) => Role::User,
            Self::Assistant(_) => Role::Assistant,
            Self::Tool(_) => Role::Tool,
            Self::ToolResult(_) => Role::ToolDisplay,
            Self::Compaction(_) => Role::Compaction,
            Self::System(_) => Role::System,
            Self::Spacer(_) => Role::Separator,
            Self::Logo(_) => Role::System,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::User(m) | Self::Assistant(m) => m.markdown.clone(),
            Self::Tool(t) => t.name.clone(),
            Self::ToolResult(r) => r.name.clone(),
            Self::Compaction(c) => c.summary.clone(),
            Self::System(s) => s.text.clone(),
            Self::Spacer(_) => String::new(),
            Self::Logo(text) => text.clone(),
        }
    }
}

impl TranscriptEntry {
    pub fn role(&self) -> Role {
        self.kind.role()
    }

    pub fn text(&self) -> String {
        self.kind.text()
    }
}

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
