use super::*;

use crate::runtime::frame_timing::FRAME_SLOW_WARN_MS;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Render one frame for a coordinator built with `side_pane_visible` and
/// `render_stats_visible`, and return the screen as plain text.
fn render_frame_text(
    side_pane_visible: Option<bool>,
    render_stats_visible: bool,
    columns: u16,
    rows: u16,
) -> Result<String> {
    let mut coordinator = RuntimeCoordinator::new(columns, rows, side_pane_visible);
    coordinator.state.render_stats_visible = render_stats_visible;
    let mut terminal = Terminal::new(TestBackend::new(columns, rows))?;
    let mut live = Some(&mut terminal);
    coordinator.render_frame(&mut live)?;

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    Ok(text)
}

/// WHEN the coordinator is constructed with the side pane requested, THE frame
/// SHALL carry the pane; WHEN it is declined or left unset, THE frame SHALL NOT.
/// The constructor stores the argument, so the pane's visibility follows it.
#[test]
fn test_runtime_coordinator_honors_side_pane_visible() -> Result<()> {
    // -- Setup & Fixtures
    const COLUMNS: u16 = 120;
    const ROWS: u16 = 30;

    // -- Exec
    let requested = render_frame_text(Some(true), false, COLUMNS, ROWS)?;
    let unset = render_frame_text(None, false, COLUMNS, ROWS)?;
    let declined = render_frame_text(Some(false), false, COLUMNS, ROWS)?;

    // -- Check
    assert!(
        requested.contains("Events"),
        "a requested side pane must render; frame:\n{requested}"
    );
    assert!(
        !unset.contains("Events"),
        "an unset side pane must not render; frame:\n{unset}"
    );
    assert!(
        !declined.contains("Events"),
        "a declined side pane must not render; frame:\n{declined}"
    );
    Ok(())
}

/// WHEN frames paint, THE coordinator's frame timing SHALL count each painted
/// frame with its last duration and SHALL NOT report a slow frame.
#[test]
fn test_runtime_coordinator_frame_timing_counts_painted_frames() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, None);
    let mut terminal = Terminal::new(TestBackend::new(120, 30))?;
    let mut live = Some(&mut terminal);

    // -- Exec
    coordinator.render_frame(&mut live)?;
    coordinator.render_frame(&mut live)?;

    // -- Check
    let timing = coordinator.frame_timing();
    assert_eq!(timing.frames_painted(), 2);
    assert_eq!(timing.slow_frames(), 0);
    assert!(
        timing.last_frame_millis() < FRAME_SLOW_WARN_MS,
        "a test-backend frame must stay under the slow threshold; took {} ms",
        timing.last_frame_millis()
    );
    Ok(())
}

/// WHEN no live terminal is attached, THE frame timing SHALL stay at zero: a
/// headless pass paints nothing and counts nothing.
#[test]
fn test_runtime_coordinator_frame_timing_ignores_headless_passes() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, None);
    let mut live: Option<&mut Terminal<TestBackend>> = None;

    // -- Exec
    coordinator.render_frame(&mut live)?;

    // -- Check
    assert_eq!(coordinator.frame_timing().frames_painted(), 0);
    assert_eq!(coordinator.frame_timing().slow_frames(), 0);
    Ok(())
}

/// WHEN the coordinator records an input arm, THE arm timing it reports SHALL
/// carry that arm's last duration and slow classification.
#[test]
fn test_runtime_coordinator_record_input_arm_forwards_to_arm_timing() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    let bound: u128 = 100;

    // -- Exec
    let under = coordinator.record_input_arm(bound - 1, bound);
    let over = coordinator.record_input_arm(bound + 20, bound);

    // -- Check
    assert!(!under, "an arm under the bound must not read as slow");
    assert!(over, "an arm over the bound must read as slow");
    let timing = coordinator.arm_timing();
    assert_eq!(timing.last_input_arm_millis(), bound + 20);
    assert_eq!(timing.last_stream_arm_millis(), 0);
    Ok(())
}

/// WHEN the coordinator records a stream arm, THE arm timing it reports SHALL
/// carry that arm alone: the input arm's duration stays untouched.
#[test]
fn test_runtime_coordinator_record_stream_arm_forwards_to_arm_timing() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    let bound: u128 = 100;

    // -- Exec
    let slow = coordinator.record_stream_arm(bound, bound);

    // -- Check
    assert!(slow, "a stream arm at the bound must read as slow");
    let timing = coordinator.arm_timing();
    assert_eq!(timing.last_stream_arm_millis(), bound);
    assert_eq!(timing.last_input_arm_millis(), 0);
    Ok(())
}

/// WHEN the render-stats band is visible, THE transcript content area SHALL be
/// exactly two rows shorter than when the band is hidden: the data row and its
/// legend row.
#[test]
fn test_runtime_coordinator_render_stats_band_shortens_the_transcript_by_two() -> Result<()> {
    // -- Setup & Fixtures: the same surface, rendered with the row hidden and
    // shown.
    const COLUMNS: u16 = 120;
    const ROWS: u16 = 30;

    // -- Exec
    let hidden = render_transcript_height(COLUMNS, ROWS, false)?;
    let shown = render_transcript_height(COLUMNS, ROWS, true)?;

    // -- Check
    assert_eq!(
        hidden,
        shown + 2,
        "a visible stats band must cost the transcript exactly two rows"
    );
    Ok(())
}

/// WHEN the side pane is visible, THE frame SHALL paint both band rows inside
/// the main column's bottom box and SHALL leave the Events pane unchanged.
#[test]
fn test_runtime_coordinator_render_stats_band_stays_inside_the_side_pane_layout() -> Result<()> {
    // -- Setup & Fixtures
    const COLUMNS: u16 = 120;
    const ROWS: u16 = 30;
    const MAIN_COLUMNS: usize = 84;

    // -- Exec
    let frame = render_frame_text(Some(true), true, COLUMNS, ROWS)?;

    // -- Check: the data row, then the legend under it, on the last two lines.
    let lines: Vec<&str> = frame.lines().collect();
    let data_row = lines
        .iter()
        .position(|line| line.contains("str:"))
        .ok_or("should find the stats data row")?;
    assert_eq!(
        data_row as u16,
        ROWS - 2,
        "the data row must sit one line above the band's bottom\n{frame}"
    );
    let legend_row = lines
        .iter()
        .position(|line| line.contains("stream fold ms"))
        .ok_or("should find the legend row")?;
    assert_eq!(
        legend_row as u16,
        ROWS - 1,
        "the legend must be the bottom box's last row\n{frame}"
    );
    for row in [data_row, legend_row] {
        let tail_column = lines[row]
            .find('m')
            .ok_or("should find the row's tail glyph")?;
        assert!(
            tail_column < MAIN_COLUMNS,
            "every band row must stay inside the main column\n{frame}"
        );
    }
    assert!(
        frame.contains("Events pane reserved"),
        "the Events pane must stay unchanged\n{frame}"
    );
    Ok(())
}

// region:    --- Clipboard

/// WHEN a copy fails, THE coordinator SHALL set the failure status message once
/// the tick folds the outcome, and SHALL NOT set it before the outcome lands.
#[tokio::test]
async fn test_runtime_coordinator_clipboard_failure_sets_the_status_message() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    coordinator.clipboard_writer = failing_writer;
    coordinator
        .state
        .input
        .set_clipboard_request("failure-payload".to_string());

    // -- Exec
    coordinator.flush_clipboard_request();
    let failed_before_fold = coordinator.state.status.message.status_line().to_string();
    let _ = settle_copy(&mut coordinator).await?;

    // -- Check
    assert!(
        failed_before_fold.is_empty(),
        "the loop task must not report before the outcome is folded, got: {failed_before_fold:?}"
    );
    assert_eq!(
        coordinator.state.status.message.status_line(),
        "Clipboard copy failed: pasteboard unavailable"
    );
    assert!(
        recorded("failure-payload"),
        "the writer must receive the text"
    );
    Ok(())
}

/// WHEN a copy succeeds, THE coordinator SHALL NOT set a status message.
#[tokio::test]
async fn test_runtime_coordinator_clipboard_success_sets_no_message() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    coordinator.clipboard_writer = ok_writer;
    coordinator
        .state
        .input
        .set_clipboard_request("success-payload".to_string());

    // -- Exec
    coordinator.flush_clipboard_request();
    let changed = settle_copy(&mut coordinator).await?;

    // -- Check
    assert!(!changed, "a successful copy changes nothing on screen");
    assert!(
        coordinator.state.status.message.status_line().is_empty(),
        "a successful copy must not set a message, got: {:?}",
        coordinator.state.status.message.status_line()
    );
    assert!(
        recorded("success-payload"),
        "the writer must receive the text"
    );
    Ok(())
}

/// WHILE a copy is in flight, THE request set meanwhile SHALL stay pending, and
/// THE loop SHALL start it only after the in-flight outcome is folded.
#[tokio::test]
async fn test_runtime_coordinator_clipboard_keeps_one_copy_in_flight() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    coordinator.clipboard_writer = slow_writer;
    coordinator
        .state
        .input
        .set_clipboard_request("first-payload".to_string());

    // -- Exec: start one copy, then ask for another while it runs.
    coordinator.flush_clipboard_request();
    coordinator
        .state
        .input
        .set_clipboard_request("second-payload".to_string());
    coordinator.flush_clipboard_request();

    // -- Check: the second request is still pending, not sent.
    assert_eq!(
        coordinator.state.input.take_clipboard_request(),
        Some("second-payload".to_string()),
        "a request set during an in-flight copy must stay pending"
    );

    // -- Exec: fold the first outcome, then flush the pending request again.
    let _ = settle_copy(&mut coordinator).await?;
    coordinator
        .state
        .input
        .set_clipboard_request("third-payload".to_string());
    coordinator.flush_clipboard_request();
    let _ = settle_copy(&mut coordinator).await?;

    // -- Check
    assert!(
        recorded("first-payload"),
        "the first copy must reach the writer"
    );
    assert!(
        recorded("third-payload"),
        "the next copy must start after the fold"
    );
    Ok(())
}

// endregion: --- Clipboard

// region:    --- Stream arm

/// WHEN a mixed batch folds, THE stream arm's totals SHALL count and time each
/// record kind separately, so the warn names the fold step that spent the arm.
#[test]
fn test_runtime_coordinator_stream_arm_aggregates_folds_by_kind() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    let batch = vec![
        SessionRecord::Inference(InferenceRecord::Chunk {
            inference: agnt_runtime::protocol::events::InferenceId::mint(),
            delta: "delta".to_string(),
            aggregated: "aggregated".to_string(),
        }),
        SessionRecord::Turn(agnt_runtime::protocol::events::TurnRecord::Completed {
            duration_ms: 0,
        }),
        SessionRecord::Inference(InferenceRecord::Stopped {
            cause: agnt_runtime::protocol::events::StopCause::Other,
            reason: "stopped".to_string(),
        }),
    ];

    // -- Exec
    coordinator.begin_stream_arm();
    let _ = coordinator.reduce_record_batch(batch);
    let line = coordinator.stream_arm_breakdown(1);

    // -- Check: one fold per kind, each with its own count (the millisecond part
    // is real wall time, so only its presence is asserted here).
    for label in ["inference-chunk=1/", "turn=1/", "inference-stopped=1/"] {
        assert!(line.contains(label), "the line must carry {label}: {line}");
    }
    assert!(line.contains("records=3"), "one record per fold: {line}");
    Ok(())
}

// endregion: --- Stream arm

// region:    --- Test Support

/// Render one frame with the render-stats row hidden or shown, and return the
/// transcript content area's height.
fn render_transcript_height(columns: u16, rows: u16, render_stats_visible: bool) -> Result<u16> {
    let mut coordinator = RuntimeCoordinator::new(columns, rows, None);
    coordinator.state.render_stats_visible = render_stats_visible;
    let mut terminal = Terminal::new(TestBackend::new(columns, rows))?;
    let mut live = Some(&mut terminal);
    coordinator.render_frame(&mut live)?;
    Ok(coordinator.state.scroll.transcript_area.height)
}

/// The texts the fake writers received. Every test asserts on its own payload,
/// so parallel tests cannot interfere.
static RECORDED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn record(text: &str) {
    let mut recorded = RECORDED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    recorded.push(text.to_string());
}

fn recorded(text: &str) -> bool {
    let recorded = RECORDED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    recorded.iter().any(|seen| seen == text)
}

fn ok_writer(text: &str) -> core::result::Result<(), String> {
    record(text);
    Ok(())
}

fn failing_writer(text: &str) -> core::result::Result<(), String> {
    record(text);
    Err("pasteboard unavailable".to_string())
}

/// A writer slow enough that the next flush still sees the copy in flight.
fn slow_writer(text: &str) -> core::result::Result<(), String> {
    std::thread::sleep(Duration::from_millis(50));
    record(text);
    Ok(())
}

fn copy_pending(coordinator: &RuntimeCoordinator) -> bool {
    coordinator.clipboard_copy.is_some()
}

/// Fold the copy the coordinator holds until it lands. Returns whether the fold
/// changed the rendered state. The blocking writer needs real time, so the wait
/// is bounded in wall-clock terms, not yields.
async fn settle_copy(coordinator: &mut RuntimeCoordinator) -> core::result::Result<bool, String> {
    for _ in 0..250 {
        if !copy_pending(coordinator) {
            return Ok(false);
        }
        let changed = coordinator.poll_clipboard_outcome().await;
        if !copy_pending(coordinator) {
            return Ok(changed);
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    Err("the copy outcome must land within half a second".to_string())
}

// endregion: --- Test Support
