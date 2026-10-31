//! Generic transcript entries: the store's data model.
//!
//! One entry per fact. A tool call is ONE [`ToolEntry`] for its whole life —
//! the entry carries the permission-gate `preview` and the completion
//! `result` in place, so no companion block is pushed.

use std::str::FromStr;

use super::ir::{Display, MessageRole, NoticeKind, ToolName};
use super::renderer::ItemStatus;
use crate::protocol::events::ToolCall;
use crate::tools::handler::builtin_kinds::BuiltinKind;

// region:    --- Types

/// How a tool entry families its rendering. Classified once, at construction,
/// from the tool name — never re-derived at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Edit,
    Nu,
    Generic,
}

impl ToolKind {
    /// Classify a call by its tool name through the builtin-kind parse. An
    /// unregistered name is `Generic`.
    pub fn from_name(name: &ToolName) -> Self {
        match BuiltinKind::from_str(&name.0) {
            Ok(BuiltinKind::Edit) => Self::Edit,
            Ok(BuiltinKind::Nu) => Self::Nu,
            _ => Self::Generic,
        }
    }
}

/// One tool call: identity, typed payload, and the displays it acquires over
/// its life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEntry {
    pub kind: ToolKind,
    pub call: ToolCall,
    /// The permission-gate display, set in place while the call waits for a
    /// decision.
    pub preview: Option<Display>,
    /// The completion display, set in place when the call finishes.
    pub result: Option<Display>,
    pub status: ItemStatus,
}

/// User or assistant prose — raw markdown, projected at render width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEntry {
    pub role: MessageRole,
    pub markdown: String,
}

/// A compaction or system notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeEntry {
    pub kind: NoticeKind,
    pub text: String,
}

/// Centered banner text (e.g. startup ASCII art).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerEntry {
    pub text: String,
}

impl ToolEntry {
    /// The display the entry renders, if it has one: the completion result
    /// takes over at completion; the preview shows while the call is pending
    /// and persists when the completion carries no display.
    pub fn display(&self) -> Option<&Display> {
        self.result.as_ref().or(self.preview.as_ref())
    }

    /// Blank rows this entry carries above and below its content.
    ///
    /// A gate preview that renders a code or diff surface is a banded region:
    /// it closes with one blank row, so the next tool call reads apart from it
    /// exactly as the pre-wave block boundary spaced it. A completion display
    /// renders bare — plain rows, or a diff body on the pane — and closes with
    /// a row only when it carries a diff body, whose tints sit on the pane.
    /// Every other entry is flush.
    pub fn padding(&self) -> (usize, usize) {
        let bottom = match (&self.result, &self.preview) {
            (None, Some(preview)) => usize::from(preview.has_code_or_diff()),
            (Some(result), _) => usize::from(result.has_diff()),
            (None, None) => 0,
        };
        (0, bottom)
    }
}

/// One transcript entry. The store holds these; the render layer consumes
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEntry {
    Message(MessageEntry),
    Tool(ToolEntry),
    Notice(NoticeEntry),
    Banner(BannerEntry),
}

impl TranscriptEntry {
    /// Blank rows this entry carries above and below its content, as
    /// `(top, bottom)`. Prose, notices, and banners are flush: the planner's
    /// pair table already carries their spacing. A tool entry adds the blank
    /// row its own display needs. The planner subtracts the adjacent paddings
    /// from the pair gap and adds the realized rows to the entry's rect, so the
    /// entry itself owns the blank rows around its content.
    pub fn padding(&self) -> (usize, usize) {
        match self {
            Self::Tool(tool) => tool.padding(),
            Self::Message(_) | Self::Notice(_) | Self::Banner(_) => (0, 0),
        }
    }
}

// endregion: --- Types

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/transcript/entry.rs"]
mod entry_test;

// endregion: --- Tests
