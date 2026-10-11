use super::ir::{BlockSource, Fill, Lane};

/// Spinner animation frames for [`ItemStatus::InProgress`]. The indicator is
/// the status type's own rendering concern, so the frames live with
/// `ItemStatus` rather than with any particular renderer.
const IN_PROGRESS_SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    InProgress,
    Done,
    Failed,
    Queued,
    Cancelled,
    Unknown,
}

impl ItemStatus {
    /// The indicator character for this status. Terminal statuses are fixed
    /// glyphs; `InProgress` cycles through [`IN_PROGRESS_SPINNER_FRAMES`], one
    /// frame every 100 ms of wall-clock time.
    pub fn indicator_char(&self, now_millis: u128) -> &'static str {
        match self {
            ItemStatus::InProgress => {
                let idx = ((now_millis / 100) % IN_PROGRESS_SPINNER_FRAMES.len() as u128) as usize;
                IN_PROGRESS_SPINNER_FRAMES[idx]
            }
            ItemStatus::Done => "✓",
            ItemStatus::Failed => "✕",
            ItemStatus::Queued => "•",
            ItemStatus::Cancelled => "✕",
            ItemStatus::Unknown => "?",
        }
    }
}

/// Per-frame data the renderer needs at render time. The block's
/// source/lane/fill are construction-time; FrameContext is per-frame.
#[derive(Debug, Clone)]
pub struct FrameContext {
    /// Terminal width for projection/wrapping.
    pub width: usize,
    /// Wall-clock milliseconds, for spinner animation frames.
    pub now_millis: u128,
    /// Whether this block holds the input cursor.
    pub cursor: bool,
    /// Whether this block is the selected row.
    pub selected: bool,
}

/// Implemented by concrete transcript item types (Message, Tool, Notice,
/// Spacer, Banner). Answers three questions that are frozen into a flat
/// `Block` at construction time. The renderer never sees the concrete type —
/// only the `Block`.
pub trait Renderable {
    fn source(&self) -> BlockSource;
    fn lane(&self) -> Lane;
    fn fill(&self) -> Fill;
}
