//! `DedupBackend`: synchronized-output frame framing and cursor-escape dedupe.

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use crossterm::cursor::SetCursorStyle;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Every escape byte and every delegated backend call lands in one ordered log,
/// so a single assertion sees the whole frame's write sequence.
type Log = Rc<RefCell<Vec<String>>>;

const BEGIN_SYNC: &str = "write:\u{1b}[?2026h";
const END_SYNC: &str = "write:\u{1b}[?2026l";

/// The escape stream the wrapper writes its own bytes to.
#[derive(Debug, Default)]
struct RecordingWriter {
    log: Log,
}

impl Write for RecordingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf).into_owned();
        self.log.borrow_mut().push(format!("write:{text}"));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The wrapped backend: records each delegated call by name.
#[derive(Debug, Default)]
struct RecordingBackend {
    log: Log,
}

impl Backend for RecordingBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let count = content.count();
        self.log.borrow_mut().push(format!("draw({count})"));
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("hide_cursor".to_string());
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("show_cursor".to_string());
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::new(0, 0))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.log.borrow_mut().push(format!(
            "set_cursor_position({}, {})",
            position.x, position.y
        ));
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("clear".to_string());
        Ok(())
    }

    fn clear_region(&mut self, _clear_type: ClearType) -> io::Result<()> {
        self.log.borrow_mut().push("clear_region".to_string());
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size::new(80, 24))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: Size::new(80, 24),
            pixels: Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("flush".to_string());
        Ok(())
    }
}

/// The `TestBackend` regression path renders through the same generic code, so
/// it satisfies the frame contract. It owns no escape stream: every method stays
/// the trait's no-op default.
impl FrameSync for ratatui::backend::TestBackend {}

// region:    --- Test Support

fn recording_backend() -> (DedupBackend<RecordingBackend, RecordingWriter>, Log) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let backend = DedupBackend::new(
        RecordingBackend { log: log.clone() },
        RecordingWriter { log: log.clone() },
    );
    (backend, log)
}

fn diff_cells(cell: &Cell, count: u16) -> impl Iterator<Item = (u16, u16, &Cell)> {
    (0..count).map(move |x| (x, 0, cell))
}

/// The bytes crossterm writes for `style`, so the assertions compare against the
/// command's own encoding rather than a hand-copied literal.
fn style_bytes(style: SetCursorStyle) -> Result<String> {
    let mut bytes: Vec<u8> = Vec::new();
    crossterm::queue!(bytes, style)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn take_log(log: &Log) -> Vec<String> {
    log.borrow_mut().drain(..).collect()
}

// endregion: --- Test Support

/// WHEN a frame is rendered, THE DedupBackend SHALL open the synchronized-output
/// block before the frame's first write and close it after the frame's last
/// write, on the same stream as the frame.
#[test]
fn test_runtime_backend_frame_sync_wraps_every_write() -> Result<()> {
    // -- Setup & Fixtures
    let (mut backend, log) = recording_backend();
    let cell = Cell::new("x");
    let style_escape = style_bytes(SetCursorStyle::SteadyBar)?;

    // -- Exec
    backend.begin_sync()?;
    backend.draw(diff_cells(&cell, 2))?;
    backend.show_cursor()?;
    backend.set_cursor_position(Position::new(3, 4))?;
    backend.set_cursor_style(SetCursorStyle::SteadyBar)?;
    backend.end_sync()?;

    // -- Check
    let log = take_log(&log);
    assert_eq!(
        log.first().map(String::as_str),
        Some(BEGIN_SYNC),
        "the begin marker must be the frame's first write; log {log:?}"
    );
    assert_eq!(
        log.last().map(String::as_str),
        Some(END_SYNC),
        "the end marker must be the frame's last write; log {log:?}"
    );
    assert_eq!(
        log.len(),
        6,
        "one marker, four frame writes, one marker; log {log:?}"
    );
    assert_eq!(
        log.get(4).map(String::as_str),
        Some(format!("write:{style_escape}").as_str()),
        "the cursor style must sit inside the frame block; log {log:?}"
    );
    Ok(())
}

/// WHEN a frame writes no cells and requests the cursor state already emitted,
/// THE DedupBackend SHALL suppress the repeated show/hide, position, and style
/// escapes, leaving only the frame markers.
#[test]
fn test_runtime_backend_suppresses_unchanged_cursor_state() -> Result<()> {
    // -- Setup & Fixtures
    let (mut backend, log) = recording_backend();
    let position = Position::new(5, 2);

    // -- Exec: the first frame emits every escape.
    backend.begin_sync()?;
    backend.draw(std::iter::empty())?;
    backend.show_cursor()?;
    backend.set_cursor_position(position)?;
    backend.set_cursor_style(SetCursorStyle::SteadyBar)?;
    backend.end_sync()?;
    let first = take_log(&log);

    // -- Exec: an identical second frame.
    backend.begin_sync()?;
    backend.draw(std::iter::empty())?;
    backend.show_cursor()?;
    backend.set_cursor_position(position)?;
    backend.set_cursor_style(SetCursorStyle::SteadyBar)?;
    backend.end_sync()?;

    // -- Check
    assert!(
        first.contains(&"show_cursor".to_string()),
        "a cold start must emit the cursor visibility; log {first:?}"
    );
    let second = take_log(&log);
    assert_eq!(
        second,
        vec![
            BEGIN_SYNC.to_string(),
            "draw(0)".to_string(),
            END_SYNC.to_string(),
        ],
        "an unchanged no-op frame must emit only the frame markers and the empty diff; log {second:?}"
    );
    Ok(())
}

/// WHEN the frame writes cells, THE DedupBackend SHALL re-emit the cursor
/// position even when its value is unchanged — a cell write moves the terminal's
/// cursor, so the position is no longer where the last escape left it.
#[test]
fn test_runtime_backend_reraises_cursor_position_after_a_cell_write() -> Result<()> {
    // -- Setup & Fixtures
    let (mut backend, log) = recording_backend();
    let cell = Cell::new("x");
    let position = Position::new(5, 2);

    // -- Exec: the first frame emits the position.
    backend.begin_sync()?;
    backend.draw(diff_cells(&cell, 1))?;
    backend.show_cursor()?;
    backend.set_cursor_position(position)?;
    backend.end_sync()?;
    let _first = take_log(&log);

    // -- Exec: an identical position after a frame that changed cells.
    backend.begin_sync()?;
    backend.draw(diff_cells(&cell, 3))?;
    backend.show_cursor()?;
    backend.set_cursor_position(position)?;
    backend.end_sync()?;

    // -- Check
    let second = take_log(&log);
    assert!(
        second.contains(&"set_cursor_position(5, 2)".to_string()),
        "a cell write must re-emit the position; log {second:?}"
    );
    assert!(
        !second.contains(&"show_cursor".to_string()),
        "the unchanged visibility must stay suppressed; log {second:?}"
    );
    Ok(())
}

/// WHEN the visibility or the style changes, THE DedupBackend SHALL emit the
/// escape; WHEN only the value repeats, THE DedupBackend SHALL suppress it.
#[test]
fn test_runtime_backend_emits_changed_visibility_and_style() -> Result<()> {
    // -- Setup & Fixtures
    let (mut backend, log) = recording_backend();
    let bar = style_bytes(SetCursorStyle::SteadyBar)?;
    let block = style_bytes(SetCursorStyle::SteadyBlock)?;

    // -- Exec
    backend.hide_cursor()?;
    backend.hide_cursor()?;
    backend.show_cursor()?;
    backend.show_cursor()?;
    backend.set_cursor_style(SetCursorStyle::SteadyBar)?;
    backend.set_cursor_style(SetCursorStyle::SteadyBar)?;
    backend.set_cursor_style(SetCursorStyle::SteadyBlock)?;

    // -- Check
    let log = take_log(&log);
    assert_eq!(
        log,
        vec![
            "hide_cursor".to_string(),
            "show_cursor".to_string(),
            format!("write:{bar}"),
            format!("write:{block}"),
        ],
        "only changes may reach the stream; log {log:?}"
    );
    Ok(())
}

/// WHEN a clear or a scroll moves the terminal cursor, THE DedupBackend SHALL
/// forget the emitted position so the next frame re-positions the cursor.
#[test]
fn test_runtime_backend_clear_forgets_the_emitted_position() -> Result<()> {
    // -- Setup & Fixtures
    let (mut backend, log) = recording_backend();
    let position = Position::new(1, 7);

    // -- Exec
    backend.set_cursor_position(position)?;
    backend.clear()?;
    backend.set_cursor_position(position)?;

    // -- Check
    let log = take_log(&log);
    assert_eq!(
        log,
        vec![
            "set_cursor_position(1, 7)".to_string(),
            "clear".to_string(),
            "set_cursor_position(1, 7)".to_string(),
        ],
        "a clear must invalidate the position dedupe; log {log:?}"
    );
    Ok(())
}

/// THE DedupBackend SHALL delegate every backend query and non-cursor write to
/// the wrapped backend unchanged.
#[test]
fn test_runtime_backend_delegates_the_wrapped_backend() -> Result<()> {
    // -- Setup & Fixtures
    let (mut backend, log) = recording_backend();
    let cell = Cell::new("x");

    // -- Exec & Check
    backend.draw(diff_cells(&cell, 2))?;
    assert_eq!(
        <DedupBackend<RecordingBackend, RecordingWriter> as Backend>::size(&backend)?,
        Size::new(80, 24),
        "the size query must reach the wrapped backend"
    );
    backend.flush()?;
    backend.clear_region(ClearType::All)?;

    let log = take_log(&log);
    assert_eq!(
        log,
        vec![
            "draw(2)".to_string(),
            "flush".to_string(),
            "clear_region".to_string(),
        ],
        "the wrapped backend must see every delegated call; log {log:?}"
    );
    Ok(())
}
