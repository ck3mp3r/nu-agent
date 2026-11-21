//! The render-stats band: the loop's own counters on one line under the bottom
//! box's status section, and the legend that decodes them on the line below.
//!
//! The drawer reads the measured data and formats it; no counter carries a
//! display string, and the legend is derived here from the same segment order.
//! One `Paragraph` per row paints the band, so a visible band costs two widgets
//! per frame and the height accounting changes only on the toggle.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::runtime::RuntimeCoordinator;
use crate::runtime::layout::STATS_BAND_ROWS;

/// The separator between two stats segments: one space, U+00B7 MIDDLE DOT, one
/// space. The fit math counts it as width 3, so a dropped leading segment drops
/// its separator with it.
const SEGMENT_SEPARATOR: &str = " · ";

/// The legend that decodes the data row: one `<name>:<meaning>` segment per data
/// segment, in the data row's order. It decodes the HUD only — the log-side
/// `@<index>` attribution belongs to the warn line, not to this row.
const LEGEND_SEGMENTS: [&str; 7] = [
    "fps: frames in last second",
    "last: paint ms",
    "slow: over 150ms",
    "in: input fold ms",
    "str: stream fold ms",
    "wait: key to fold ms",
    "tick: tick lateness ms",
];

impl RuntimeCoordinator {
    /// Paint the render-stats band into `band`: the data row first (five
    /// measured segments), then the legend row directly under it. Both rows are
    /// right-aligned in the theme's meta style and drop leading segments on
    /// their own; a row whose first segment cannot fit paints nothing.
    pub(crate) fn render_stats_band(&mut self, frame: &mut Frame, band: Rect) {
        let width = usize::from(band.width);
        if let Some(line) = self.stats_line(width)
            && band.height > 0
        {
            frame.render_widget(
                Paragraph::new(line)
                    .style(self.theme.subtle_meta)
                    .alignment(Alignment::Right),
                Rect { height: 1, ..band },
            );
        }
        if let Some(line) = Self::fit_segments(&LEGEND_SEGMENTS.map(str::to_string), width)
            && band.height >= STATS_BAND_ROWS
        {
            frame.render_widget(
                Paragraph::new(line)
                    .style(self.theme.subtle_meta)
                    .alignment(Alignment::Right),
                Rect {
                    y: band.y.saturating_add(1),
                    height: 1,
                    ..band
                },
            );
        }
    }

    /// The stats line that fits `width` columns. Segments drop from the front
    /// (the fps reading first) until the line fits; `None` when even the last
    /// segment is too wide, so the row keeps its background.
    fn stats_line(&mut self, width: usize) -> Option<Line<'static>> {
        let fps = self.fps();
        let timing = self.frame_timing();
        let arms = self.arm_timing();
        let segments = [
            format!("fps: {fps}"),
            format!("last: {}ms", timing.last_frame_millis()),
            format!("slow: {}", timing.slow_frames()),
            format!("in: {}ms", arms.last_input_arm_millis()),
            format!("str: {}ms", arms.last_stream_arm_millis()),
            format!("wait: {}ms", arms.last_input_wait_millis()),
            format!("tick: {}ms", arms.last_tick_late_millis()),
        ];
        Self::fit_segments(&segments, width)
    }

    /// The one-row line the segments make inside `width` columns: leading
    /// segments drop until the rest fits, and `None` means not even the tail
    /// segment fits. Both band rows obey this rule independently.
    fn fit_segments(segments: &[String; 7], width: usize) -> Option<Line<'static>> {
        let mut first = 0;
        while first < segments.len() {
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(segments.len() * 2 - 1);
            for (index, segment) in segments[first..].iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw(SEGMENT_SEPARATOR));
                }
                spans.push(Span::raw(segment.clone()));
            }
            let line = Line::from(spans);
            let needed: usize = line
                .spans
                .iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum();
            if needed <= width {
                return Some(line);
            }
            first += 1;
        }
        None
    }
}
