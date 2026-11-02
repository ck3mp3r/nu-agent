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

/// User or assistant prose — raw markdown, projected at render width, plus the
/// footer facts the entry's producer holds.
///
/// The three footer slots carry exactly what their producer knows and nothing
/// else: no fallback fill, no invented value. A slot stays `None` where the fact
/// is unknown or was never recorded (a `0` stamp, an empty or whitespace
/// identity is unknown), so an entry whose three slots are `None` renders
/// footerless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEntry {
    pub role: MessageRole,
    pub markdown: String,
    /// When the entry was made, in Unix milliseconds. `i64`, the storage
    /// engine's own stamp type: no casts between the two.
    pub timestamp_millis: Option<i64>,
    /// The model identity the entry was produced with, as recorded.
    pub model: Option<String>,
    /// The agent identity the entry was produced with, as recorded.
    pub agent: Option<String>,
}

/// A compaction or system notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeEntry {
    pub kind: NoticeKind,
    pub text: String,
}

/// One background task's call line: the task, the subject it works on, and the
/// status its glyph carries.
///
/// One entry for the task's whole life, like [`ToolEntry`]: the line is pushed
/// once, and finishing mutates its `status` in place. What the task produces —
/// a compaction's streamed summary — renders below the line as its own entries,
/// so the call line itself is stable except for its glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEntry {
    /// The task's name, as the line's emphasis: `Compaction`.
    pub name: String,
    /// The subject the task works on, as the line's muted tail: the compaction
    /// source, `auto`.
    pub summary: String,
    /// Where the task stands. The glyph animates from this in place.
    pub status: ItemStatus,
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

impl MessageEntry {
    /// Whether the entry has any footer fact to show. False only when all three
    /// slots are absent: the entry then renders footerless.
    pub fn has_footer(&self) -> bool {
        self.timestamp_millis.is_some() || self.model.is_some() || self.agent.is_some()
    }
}

/// One transcript entry. The store holds these; the render layer consumes
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEntry {
    Message(MessageEntry),
    Tool(ToolEntry),
    Task(TaskEntry),
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
            Self::Message(_) | Self::Task(_) | Self::Notice(_) | Self::Banner(_) => (0, 0),
        }
    }
}

// endregion: --- Types

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/transcript/entry.rs"]
mod entry_test;

// endregion: --- Tests
