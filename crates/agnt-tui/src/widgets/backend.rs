//! The ratatui backend: one buffer, one dispatch by entry family.
//!
//! The backend owns the target buffer and paints each entry's rect into it.
//! The entry enum selects the family widget; dispatch stays static. The pane
//! resets the buffer once per frame and blits it to the terminal.

use agnt_core::transcript::entry::TranscriptEntry;
use agnt_render::{RenderBackend, RowRect, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::banner::BannerWidget;
use super::message::MessageWidget;
use super::notice::NoticeWidget;
use super::tool::ToolWidget;
use super::{PaintCtx, PaneWidget};

// region:    --- Types

/// The ratatui rendering backend. Row rects are absolute frame rows, so the
/// buffer is the frame-sized surface the pane blits.
#[derive(Debug)]
pub struct RatatuiBackend {
    buffer: Buffer,
    now_millis: u128,
}

// endregion: --- Types

// region:    --- Constructors

impl RatatuiBackend {
    /// A backend over a blank `width` x `height` buffer.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            now_millis: 0,
        }
    }

    /// The painted surface.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Drop every painted cell, so the backend can paint a fresh frame.
    pub fn clear(&mut self) {
        self.buffer.reset();
    }

    /// Set the frame's clock reading for the status spinner.
    pub fn set_now_millis(&mut self, now_millis: u128) {
        self.now_millis = now_millis;
    }
}

// endregion: --- Constructors

// region:    --- RenderBackend

impl RenderBackend for RatatuiBackend {
    fn render(&mut self, entry: &TranscriptEntry, rect: RowRect, theme: &Theme) {
        let Some(area) = frame_area(rect) else {
            return;
        };
        reset_area(&mut self.buffer, area);
        let ctx = PaintCtx {
            theme,
            now_millis: self.now_millis,
        };
        match entry {
            TranscriptEntry::Message(message) => MessageWidget {
                entry: message,
                content_offset: rect.content_offset,
            }
            .paint(area, &mut self.buffer, ctx),
            TranscriptEntry::Tool(tool) => {
                ToolWidget { entry: tool }.paint(area, &mut self.buffer, ctx)
            }
            TranscriptEntry::Notice(notice) => {
                NoticeWidget { entry: notice }.paint(area, &mut self.buffer, ctx)
            }
            TranscriptEntry::Banner(banner) => {
                BannerWidget { entry: banner }.paint(area, &mut self.buffer, ctx)
            }
        }
    }

    fn measure(&self, entry: &TranscriptEntry, width: usize) -> usize {
        match entry {
            TranscriptEntry::Message(message) => MessageWidget {
                entry: message,
                content_offset: 0,
            }
            .measure(width),
            TranscriptEntry::Tool(tool) => ToolWidget { entry: tool }.measure(width),
            TranscriptEntry::Notice(notice) => NoticeWidget { entry: notice }.measure(width),
            TranscriptEntry::Banner(banner) => BannerWidget { entry: banner }.measure(width),
        }
    }
}

// endregion: --- RenderBackend

// region:    --- Support

/// The frame area for a row rect, or `None` when the rect paints nothing.
fn frame_area(rect: RowRect) -> Option<Rect> {
    if rect.rows == 0 || rect.width == 0 {
        return None;
    }
    Some(Rect::new(
        0,
        u16::try_from(rect.row).unwrap_or(u16::MAX),
        u16::try_from(rect.width).unwrap_or(u16::MAX),
        u16::try_from(rect.rows).unwrap_or(u16::MAX),
    ))
}

/// Blank every cell of `area`, so a shorter re-render leaves no stale cells.
fn reset_area(buf: &mut Buffer, area: Rect) {
    let bottom = area.bottom().min(buf.area.bottom());
    let right = area.right().min(buf.area.right());
    for y in area.y..bottom {
        for x in area.left()..right {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
    }
}

// endregion: --- Support
