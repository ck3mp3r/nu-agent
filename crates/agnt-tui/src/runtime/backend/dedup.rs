//! A backend wrapper that frames each render pass as one synchronized-output
//! update and suppresses cursor escapes that only repeat emitted state.

use std::io::{self, Write};

use crossterm::cursor::SetCursorStyle;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

// region:    --- Types

/// DEC private mode 2026 — synchronized output. A terminal that implements it
/// buffers the enclosed writes and paints them as one update, so a frame's
/// intermediate states never reach the screen. A terminal that does not ignores
/// the unknown private mode and paints each write as it arrives, which is
/// exactly what it did before.
const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
const END_SYNC: &[u8] = b"\x1b[?2026l";

/// The frame-level operations a render pass needs from its backend.
///
/// Every method defaults to a no-op, so a backend that owns no escape stream of
/// its own — a test backend — satisfies the same render path without changes.
pub(crate) trait FrameSync {
    /// Open the frame's synchronized-output block. Call this before the frame's
    /// first write.
    fn begin_sync(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Emit `style` on the frame's stream, unless it is already the emitted
    /// cursor style.
    fn set_cursor_style(&mut self, _style: SetCursorStyle) -> io::Result<()> {
        Ok(())
    }

    /// Close the frame's synchronized-output block. Call this after the frame's
    /// last write.
    fn end_sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The state the wrapper last put on the wire, so a repeat is not re-sent.
#[derive(Debug, Default)]
struct Emitted {
    /// Whether the cursor is currently shown.
    shown: Option<bool>,
    /// Where the cursor was last left. `None` means "unknown", which forces the
    /// next positioning escape.
    position: Option<Position>,
    /// The cursor shape last written.
    style: Option<SetCursorStyle>,
}

/// A backend that writes its own control escapes through a second handle on the
/// same stream, and drops the ones that would repeat the emitted state.
///
/// The wrapper holds its writer separately because a `CrosstermBackend` exposes
/// no writer accessor. Both handles reach the same file descriptor, and the
/// render path is single-threaded and sequential, so write ordering holds.
pub(crate) struct DedupBackend<B: Backend, W: Write = std::io::Stderr> {
    inner: B,
    writer: W,
    emitted: Emitted,
}

// endregion: --- Types

// region:    --- DedupBackend

impl<B: Backend, W: Write> DedupBackend<B, W> {
    pub(crate) fn new(inner: B, writer: W) -> Self {
        Self {
            inner,
            writer,
            emitted: Emitted::default(),
        }
    }

    /// Forget where the cursor was left, so the next positioning escape is
    /// written. Any escape that moves the terminal's cursor invalidates the
    /// record.
    fn forget_position(&mut self) {
        self.emitted.position = None;
    }

    fn write_escape(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }
}

// endregion: --- DedupBackend

// region:    --- Backend

impl<B: Backend, W: Write> Backend for DedupBackend<B, W> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut wrote = false;
        let result = self.inner.draw(content.inspect(|_| wrote = true));
        if wrote {
            self.forget_position();
        }
        result
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.forget_position();
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        if self.emitted.shown == Some(false) {
            return Ok(());
        }
        self.emitted.shown = Some(false);
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        if self.emitted.shown == Some(true) {
            return Ok(());
        }
        self.emitted.shown = Some(true);
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        if self.emitted.position == Some(position) {
            return Ok(());
        }
        self.emitted.position = Some(position);
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.forget_position();
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.forget_position();
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

// endregion: --- Backend

// region:    --- FrameSync

impl<B: Backend, W: Write> FrameSync for DedupBackend<B, W> {
    fn begin_sync(&mut self) -> io::Result<()> {
        self.write_escape(BEGIN_SYNC)
    }

    fn set_cursor_style(&mut self, style: SetCursorStyle) -> io::Result<()> {
        if self.emitted.style == Some(style) {
            return Ok(());
        }
        self.emitted.style = Some(style);
        crossterm::queue!(self.writer, style)?;
        self.writer.flush()
    }

    fn end_sync(&mut self) -> io::Result<()> {
        self.write_escape(END_SYNC)
    }
}

// endregion: --- FrameSync
