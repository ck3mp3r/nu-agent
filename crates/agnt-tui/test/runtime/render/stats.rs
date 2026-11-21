//! The render-stats band: placement under the status section, right alignment,
//! the width-based segment drop on each row, the legend row, and the hidden
//! frame.

use super::bottom_box_support::*;
use crate::runtime::RuntimeCoordinator;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

/// Columns and rows of the golden surface.
const COLUMNS: u16 = 120;
const ROWS: u16 = 30;

/// The seven data segment labels, in paint order.
const SEGMENTS: [&str; 7] = ["fps:", "last:", "slow:", "in:", "str:", "wait:", "tick:"];

/// The legend's seven segments, in the data row's order. Duplicated here on
/// purpose: the test pins the drawer's decoding, so a changed legend fails.
const LEGEND_SEGMENTS: [&str; 7] = [
    "fps: frames in last second",
    "last: paint ms",
    "slow: over 150ms",
    "in: input fold ms",
    "str: stream fold ms",
    "wait: key to fold ms",
    "tick: tick lateness ms",
];

/// The legend row's full text: the seven segments joined by the middle dot.
const LEGEND_LINE: &str = "fps: frames in last second · last: paint ms · \
slow: over 150ms · in: input fold ms · str: stream fold ms · wait: key to fold ms · \
tick: tick lateness ms";

/// WHEN the band is visible, THE bottom box SHALL paint the data row directly
/// under the info row and the legend row directly under it, carrying every data
/// segment in paint order.
#[tokio::test]
async fn test_render_stats_band_sits_under_the_info_row_at_the_screen_bottom() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(COLUMNS, ROWS);
    driver.coordinator_mut().state.render_stats_visible = true;
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let info_row = lines
        .iter()
        .position(|line| line.contains("gpt-5"))
        .ok_or("should find the status info row")?;
    let data_row = lines
        .iter()
        .position(|line| line.contains(SEGMENTS[0]))
        .ok_or("should find the stats data row")?;
    let legend_row = lines
        .iter()
        .position(|line| line.contains(LEGEND_SEGMENTS[6]))
        .ok_or("should find the legend row")?;
    assert_eq!(
        data_row,
        info_row + 1,
        "the data row must sit directly under the info row\n{buffer}"
    );
    assert_eq!(
        legend_row,
        data_row + 1,
        "the legend must sit directly under the data row\n{buffer}"
    );
    assert_eq!(
        legend_row as u16,
        ROWS - 1,
        "the legend must be the screen's last line\n{buffer}"
    );
    let row = lines[data_row];
    let mut from = 0;
    for label in SEGMENTS {
        let at = row[from..]
            .find(label)
            .map(|index| index + from)
            .ok_or("every stats segment must appear, in paint order")?;
        from = at + label.len();
    }
    Ok(())
}

/// WHEN the band paints, THE drawer SHALL right-align both rows one column
/// inside the bottom box and SHALL paint them in the theme's meta style.
#[tokio::test]
async fn test_render_stats_band_rows_align_right_in_the_meta_style() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(COLUMNS, ROWS);
    driver.coordinator_mut().state.render_stats_visible = true;

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: the input box border row gives the bottom box's columns.
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let data_row = lines
        .iter()
        .position(|line| line.contains(SEGMENTS[6]))
        .ok_or("should find the stats data row")?;
    let legend_row = lines
        .iter()
        .position(|line| line.contains(LEGEND_SEGMENTS[6]))
        .ok_or("should find the legend row")?;
    let border = lines[input_top_row(&driver)? as usize];
    let box_left = border
        .chars()
        .position(|c| c == '╭')
        .ok_or("should find the input box left border")?;
    let box_right = border
        .chars()
        .position(|c| c == '╮')
        .ok_or("should find the input box right border")?;
    let theme = driver.coordinator().theme.clone();
    for row in [data_row, legend_row] {
        let row_chars: Vec<char> = lines[row].chars().collect();
        let last_glyph = row_chars
            .iter()
            .rposition(|c| *c != ' ')
            .ok_or("a band row must paint a glyph")?;
        let last_col: usize = row_chars[..last_glyph]
            .iter()
            .map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        let first_glyph = row_chars
            .iter()
            .position(|c| *c != ' ')
            .ok_or("a band row must paint a glyph")?;
        assert_eq!(
            last_col + 1,
            box_right,
            "each band row must end one column inside the bottom box\n{buffer}"
        );
        assert!(
            first_glyph > box_left + 1,
            "each band row must be right-aligned, not flush left\n{buffer}"
        );
        let Ok(glyph_col) = u16::try_from(last_col) else {
            return Err("the band rows must fit the surface".into());
        };
        assert_eq!(
            driver.cell_style_at(glyph_col, row as u16).fg,
            theme.subtle_meta.fg,
            "each band row must paint in the theme's meta style"
        );
    }
    Ok(())
}

/// WHEN the bottom box leaves fewer columns than the formatted line needs, THE
/// drawer SHALL drop the leading frame count from the data row first.
#[tokio::test]
async fn test_render_stats_band_drops_the_frame_count_first_when_narrow() -> Result<()> {
    // -- Setup & Fixtures: 40 columns leave 36 status columns, fewer than the
    // five segments need on one line.
    let mut driver = RenderLoopDriver::new(40, ROWS);
    driver.coordinator_mut().state.render_stats_visible = true;

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    let row = buffer
        .lines()
        .find(|line| line.contains(SEGMENTS[6]))
        .ok_or("the tail segment must stay on screen")?;
    assert!(
        !row.contains(SEGMENTS[0]),
        "the leading frame count must drop first\n{buffer}"
    );
    assert!(
        row.contains(SEGMENTS[6]),
        "the remaining segments must stay on screen\n{buffer}"
    );
    Ok(())
}

/// WHILE `render_stats_visible` is false, THE frame SHALL paint no band glyph
/// and THE status info row SHALL stay the screen's last line.
#[tokio::test]
async fn test_render_stats_band_hidden_frame_keeps_the_status_row_last() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(COLUMNS, ROWS);
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let info_row = lines
        .iter()
        .position(|line| line.contains("gpt-5"))
        .ok_or("should find the status info row")?;
    assert_eq!(
        info_row as u16,
        ROWS - 1,
        "the info row must stay the screen's last line\n{buffer}"
    );
    assert!(
        !buffer.contains(SEGMENTS[0]) && !buffer.contains("painted"),
        "a hidden band must paint neither row\n{buffer}"
    );
    Ok(())
}

// region:    --- Test Support

/// The fixture line for a coordinator with a recorded 7 ms input arm, a
/// recorded 3 ms stream arm, an 11 ms input wait, a 5 ms tick lateness, and no
/// painted frame.
const FULL_LINE: &str =
    "fps: 0 · last: 0ms · slow: 0 · in: 7ms · str: 3ms · wait: 11ms · tick: 5ms";

/// A coordinator whose segment values are exact: recorded arm, wait, and tick
/// measurements, no painted frame.
fn drawer_coordinator() -> RuntimeCoordinator {
    let mut coordinator = RuntimeCoordinator::new(COLUMNS, 1, None);
    coordinator.record_input_arm(7, 1000);
    coordinator.record_stream_arm(3, 1000);
    coordinator.record_input_wait(11, 1000);
    coordinator.record_tick_late(5, 1000);
    coordinator
}

/// Paint the band into a two-row rect `width` columns wide, and return the data
/// row and the legend row as plain text.
fn draw_stats_band(coordinator: &mut RuntimeCoordinator, width: u16) -> Result<(String, String)> {
    let mut terminal = Terminal::new(TestBackend::new(width, 2))?;
    terminal.draw(|frame| {
        coordinator.render_stats_band(frame, Rect::new(0, 0, width, 2));
    })?;
    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| {
        let mut text = String::new();
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text
    };
    Ok((row_text(0), row_text(1)))
}

// endregion: --- Test Support

/// WHEN the band is at least as wide as the data line, THE drawer SHALL paint
/// all seven data segments joined by two spaces.
#[test]
fn test_render_stats_band_paints_all_data_segments_that_fit() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = drawer_coordinator();
    let width = u16::try_from(UnicodeWidthStr::width(FULL_LINE))?;

    // -- Exec
    let (data_row, legend_row) = draw_stats_band(&mut coordinator, width)?;

    // -- Check: the data row is byte-identical to the one-row format, and the
    // legend still paints its tail in the same width.
    assert_eq!(data_row, FULL_LINE);
    assert!(
        legend_row.trim_end().ends_with(LEGEND_SEGMENTS[6]),
        "the legend keeps its tail segment: {legend_row:?}"
    );
    Ok(())
}

/// WHEN the band is at least as wide as the legend, THE legend row SHALL decode
/// all seven segments in the data row's order, and SHALL NOT mention the
/// log-side attribution suffix.
#[test]
fn test_render_stats_band_legend_decodes_every_segment_in_order() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = drawer_coordinator();
    let width = u16::try_from(UnicodeWidthStr::width(LEGEND_LINE))?;

    // -- Exec
    let (_, legend_row) = draw_stats_band(&mut coordinator, width)?;

    // -- Check
    assert_eq!(legend_row, LEGEND_LINE, "the legend decodes every segment");
    let mut from = 0;
    for segment in LEGEND_SEGMENTS {
        let at = legend_row[from..]
            .find(segment)
            .map(|index| index + from)
            .ok_or("the legend must carry every segment in the data row's order")?;
        from = at + segment.len();
    }
    assert!(
        !legend_row.contains('@'),
        "the legend must not mention the log-side suffix"
    );
    Ok(())
}

/// WHEN a row is narrower than its text, THE drawer SHALL drop that row's
/// leading segments until it fits, keeping it right-aligned.
#[test]
fn test_render_stats_band_drops_leading_segments_until_the_row_fits() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = drawer_coordinator();
    let without_leading = "str: 3ms · wait: 11ms · tick: 5ms";
    let width = 40u16;

    // -- Exec
    let (data_row, _) = draw_stats_band(&mut coordinator, width)?;

    // -- Check
    let padding = usize::from(width).saturating_sub(UnicodeWidthStr::width(without_leading));
    assert_eq!(
        data_row,
        format!("{}{without_leading}", " ".repeat(padding))
    );
    Ok(())
}

/// WHEN a row fits only its trailing segments, THE drawer SHALL keep the tail
/// and drop every leading segment of that row.
#[test]
fn test_render_stats_band_keeps_only_the_tail_when_the_row_narrows() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = drawer_coordinator();
    let tail = "str: 3ms · wait: 11ms · tick: 5ms";
    let width = u16::try_from(UnicodeWidthStr::width(tail))?;

    // -- Exec
    let (data_row, legend_row) = draw_stats_band(&mut coordinator, width)?;

    // -- Check: the data row keeps exactly its tail; the legend drops its own
    // leading segments and keeps its tail.
    assert_eq!(data_row, tail);
    assert!(
        legend_row.trim_end().ends_with(LEGEND_SEGMENTS[6]),
        "the legend keeps its tail: {legend_row:?}"
    );
    Ok(())
}

/// WHEN not even the first segment of a row fits, THE drawer SHALL paint
/// nothing for that row, so it keeps its background.
#[test]
fn test_render_stats_band_paints_nothing_when_no_segment_fits() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = drawer_coordinator();
    let width = 6u16;

    // -- Exec
    let mut terminal = Terminal::new(TestBackend::new(width, 2))?;
    terminal.draw(|frame| {
        coordinator.render_stats_band(frame, Rect::new(0, 0, width, 2));
    })?;

    // -- Check: an untouched terminal row is the reference for "not painted".
    let mut reference = Terminal::new(TestBackend::new(width, 2))?;
    reference.draw(|_frame| {})?;
    for y in 0..2 {
        let reference_style = reference.backend().buffer()[(0, y)].style();
        let buffer = terminal.backend().buffer();
        for x in 0..width {
            assert_eq!(buffer[(x, y)].symbol(), " ", "row {y} must stay blank");
            assert_eq!(
                buffer[(x, y)].style(),
                reference_style,
                "an unpainted row must keep the terminal's own style"
            );
        }
    }
    Ok(())
}

/// WHEN the rect is shorter than the band's accounting, THE drawer SHALL paint
/// only the rows the rect holds — the band is never taller than its accounting.
#[test]
fn test_render_stats_band_paints_only_the_rows_the_rect_holds() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = drawer_coordinator();
    let width = u16::try_from(UnicodeWidthStr::width(LEGEND_LINE))?;

    // -- Exec: a one-row rect, as a band whose accounting was not raised.
    let mut terminal = Terminal::new(TestBackend::new(width, 1))?;
    terminal.draw(|frame| {
        coordinator.render_stats_band(frame, Rect::new(0, 0, width, 1));
    })?;
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for x in 0..buffer.area.width {
        text.push_str(buffer[(x, 0)].symbol());
    }

    // -- Check: the data row paints, and the legend has no row to paint on.
    assert!(
        text.trim_end().ends_with("tick: 5ms"),
        "the data row must paint into the one row it has: {text:?}"
    );
    assert!(
        !text.contains("frames in last second"),
        "the legend must not paint outside the rect: {text:?}"
    );
    Ok(())
}
