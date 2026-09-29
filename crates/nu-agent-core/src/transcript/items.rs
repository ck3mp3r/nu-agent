use super::ir::{BlockSource, Fill, Lane, MessageRole, NoticeKind, StyleHint, ToolName};
use super::renderer::Renderable;
use crate::protocol::tool_args::CallLine;
use crate::tools::handler::builtin_kinds::BuiltinKind;

// Re-export the tool block type for the TUI's constructors.
pub use super::ir::Tool;

impl ToolName {
    /// Whether this name identifies the built-in `edit` tool. Tool-kind
    /// knowledge lives in the tool layer (`items.rs`), not in the transcript
    /// IR (`ir.rs`) — so the IR never imports `BuiltinKind`. Typed identity,
    /// never a title-text prefix probe.
    pub fn is_edit(&self) -> bool {
        matches!(self.0.parse::<BuiltinKind>(), Ok(BuiltinKind::Edit))
    }

    /// Whether this name identifies the built-in `nu` tool. Same typed
    /// identity rule as [`ToolName::is_edit`]: the name type owns its own
    /// classification, so the transcript IR never imports `BuiltinKind`.
    pub fn is_nu(&self) -> bool {
        matches!(self.0.parse::<BuiltinKind>(), Ok(BuiltinKind::Nu))
    }
}

/// Markdown-projected prose authored by the user or assistant. The role is
/// carried on the type, so the same struct drives both the user lane/fill and
/// the assistant lane/fill. Stores raw markdown source; projection to lines
/// happens at render time so the canvas width can be taken into account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: MessageRole,
    pub markdown: String,
}

impl Renderable for Message {
    fn source(&self) -> BlockSource {
        BlockSource::Markdown {
            role: self.role,
            markdown: self.markdown.clone(),
        }
    }

    fn lane(&self) -> Lane {
        match self.role {
            MessageRole::User => Lane::Marker("▏"),
            MessageRole::Assistant => Lane::Blank,
        }
    }

    fn fill(&self) -> Fill {
        match self.role {
            MessageRole::User => Fill::Full,
            MessageRole::Assistant => Fill::None,
        }
    }
}

/// A compaction or system notice block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

impl Renderable for Notice {
    fn source(&self) -> BlockSource {
        BlockSource::Notice {
            kind: self.kind,
            text: self.text.clone(),
        }
    }

    fn lane(&self) -> Lane {
        match self.kind {
            NoticeKind::Compaction => Lane::Marker("~"),
            NoticeKind::System => Lane::Marker("·"),
        }
    }

    fn fill(&self) -> Fill {
        Fill::None
    }
}

/// A plain system text line rendered on the system lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemMessage {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spacer;

impl Renderable for Spacer {
    fn source(&self) -> BlockSource {
        BlockSource::Spacer
    }

    fn lane(&self) -> Lane {
        Lane::Blank
    }

    fn fill(&self) -> Fill {
        Fill::None
    }
}

/// Centered banner text (e.g. startup ASCII art), one row per line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
}

impl Renderable for Banner {
    fn source(&self) -> BlockSource {
        BlockSource::Banner {
            text: self.text.clone(),
        }
    }

    fn lane(&self) -> Lane {
        // System styling, blank prefix. The lane variant carries the styling
        // so the renderer needs no source inspection (task 46ca79fe).
        Lane::SystemBlank
    }

    fn fill(&self) -> Fill {
        Fill::None
    }
}

/// Fallback call line for tools without a tailored summary: the
/// arrow-prefixed, truncated JSON arguments.
impl CallLine {
    pub fn from_json_summary(arguments: &str) -> Self {
        Self {
            summary: format!(
                "→ {}",
                crate::protocol::tool_args::summarize_tool_arguments(arguments)
            ),
        }
    }
}

impl Renderable for Tool {
    fn source(&self) -> BlockSource {
        BlockSource::Tool {
            name: self.name.clone(),
            call: self.call.clone(),
            preview: self.preview.clone(),
        }
    }

    fn lane(&self) -> Lane {
        Lane::Marker("⚙")
    }

    fn fill(&self) -> Fill {
        match &self.preview {
            Some(display) if display.has_code_or_diff() => Fill::Code,
            _ => Fill::None,
        }
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
