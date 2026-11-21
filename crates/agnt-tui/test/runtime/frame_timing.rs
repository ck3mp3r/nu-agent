use super::*;

use std::time::Duration;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The warn bound the arm records are classified against in these tests.
const ARM_WARN_MS: u128 = 100;

/// The bound an input wait is classified against.
const WAIT_WARN_MS: u128 = 250;

/// The bound a render tick's lateness is classified against.
const TICK_LATE_WARN_MS: u128 = 300;

/// WHEN a frame's elapsed time stays under one spinner cadence, THE frame-timing
/// record SHALL count the frame as painted and SHALL NOT mark it slow.
#[test]
fn test_runtime_frame_timing_below_threshold_is_not_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = FrameTiming::default();
    let just_under: u128 = FRAME_SLOW_WARN_MS - 1;

    // -- Exec
    let slow = timing.record_frame(
        Duration::from_millis(just_under as u64),
        FrameKind::Recomposed,
        0,
    );

    // -- Check
    assert!(
        !slow,
        "a frame under {FRAME_SLOW_WARN_MS} ms must not read as slow"
    );
    assert_eq!(timing.frames_painted(), 1);
    assert_eq!(timing.slow_frames(), 0);
    assert_eq!(timing.last_frame_millis(), just_under);
    Ok(())
}

/// WHEN a frame's elapsed time reaches one spinner cadence, THE frame-timing
/// record SHALL mark it slow; above the threshold it SHALL keep marking it.
#[test]
fn test_runtime_frame_timing_at_and_above_threshold_is_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = FrameTiming::default();

    // -- Exec & Check
    for elapsed_ms in [FRAME_SLOW_WARN_MS, FRAME_SLOW_WARN_MS + 1, 1_000] {
        assert!(
            timing.record_frame(
                Duration::from_millis(elapsed_ms as u64),
                FrameKind::Recomposed,
                0
            ),
            "a frame of {elapsed_ms} ms must read as slow"
        );
        assert_eq!(timing.last_frame_millis(), elapsed_ms);
    }
    assert_eq!(timing.frames_painted(), 3);
    assert_eq!(timing.slow_frames(), 3);
    Ok(())
}

/// WHEN frames of mixed cost paint, THE frame-timing record SHALL count every
/// frame, count only the slow ones as slow, and carry the last duration.
#[test]
fn test_runtime_frame_timing_counts_painted_and_slow_frames() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = FrameTiming::default();

    // -- Exec
    timing.record_frame(Duration::from_millis(2), FrameKind::Recomposed, 0);
    timing.record_frame(
        Duration::from_millis(FRAME_SLOW_WARN_MS as u64 + 40),
        FrameKind::Recomposed,
        0,
    );
    timing.record_frame(Duration::from_millis(7), FrameKind::ClockCells, 0);

    // -- Check
    assert_eq!(timing.frames_painted(), 3);
    assert_eq!(timing.slow_frames(), 1);
    assert_eq!(timing.last_frame_millis(), 7);
    Ok(())
}

/// WHEN frames of each kind paint, THEN THE frame-timing record SHALL count
/// every frame, count the full recompositions and the clock-only repaints
/// apart, and leave each other's count alone.
#[test]
fn test_runtime_frame_timing_counts_each_frame_kind() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = FrameTiming::default();

    // -- Exec
    timing.record_frame(Duration::from_millis(9), FrameKind::Recomposed, 0);
    timing.record_frame(Duration::from_millis(1), FrameKind::ClockCells, 0);
    timing.record_frame(Duration::from_millis(1), FrameKind::ClockCells, 0);

    // -- Check
    assert_eq!(timing.frames_painted(), 3);
    assert_eq!(timing.recomposed_frames(), 1);
    assert_eq!(timing.clock_frames(), 2);
    Ok(())
}

/// WHEN an input arm stays under the warn bound, THE arm-timing record SHALL
/// carry its duration and SHALL NOT mark it slow, leaving the stream arm's
/// duration alone.
#[test]
fn test_runtime_arm_timing_input_below_bound_is_not_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec
    let slow = timing.record_input_arm(ARM_WARN_MS - 1, ARM_WARN_MS);

    // -- Check
    assert!(!slow, "an arm under {ARM_WARN_MS} ms must not read as slow");
    assert_eq!(timing.last_input_arm_millis(), ARM_WARN_MS - 1);
    assert_eq!(timing.last_stream_arm_millis(), 0);
    Ok(())
}

/// WHEN an input arm reaches the warn bound, THE arm-timing record SHALL mark
/// it slow; above the bound it SHALL keep marking it.
#[test]
fn test_runtime_arm_timing_input_at_and_above_bound_is_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec & Check
    for elapsed_ms in [ARM_WARN_MS, ARM_WARN_MS + 1, 1_000] {
        assert!(
            timing.record_input_arm(elapsed_ms, ARM_WARN_MS),
            "an input arm of {elapsed_ms} ms must read as slow"
        );
        assert_eq!(timing.last_input_arm_millis(), elapsed_ms);
    }
    Ok(())
}

/// WHEN a stream arm reaches the warn bound, THE arm-timing record SHALL mark
/// it slow and keep the input arm's duration untouched.
#[test]
fn test_runtime_arm_timing_stream_at_and_above_bound_is_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec
    let under = timing.record_stream_arm(ARM_WARN_MS - 1, ARM_WARN_MS);
    let at = timing.record_stream_arm(ARM_WARN_MS, ARM_WARN_MS);

    // -- Check
    assert!(!under, "a stream arm under {ARM_WARN_MS} ms is not slow");
    assert!(at, "a stream arm at {ARM_WARN_MS} ms is slow");
    assert_eq!(timing.last_stream_arm_millis(), ARM_WARN_MS);
    assert_eq!(timing.last_input_arm_millis(), 0);
    Ok(())
}

/// WHEN both arms of mixed cost complete, THE arm-timing record SHALL carry
/// each arm's own last duration.
#[test]
fn test_runtime_arm_timing_carries_each_arms_last_duration() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec
    timing.record_input_arm(4, ARM_WARN_MS);
    timing.record_stream_arm(ARM_WARN_MS + 20, ARM_WARN_MS);
    timing.record_input_arm(6, ARM_WARN_MS);

    // -- Check
    assert_eq!(timing.last_input_arm_millis(), 6);
    assert_eq!(timing.last_stream_arm_millis(), ARM_WARN_MS + 20);
    Ok(())
}

/// WHEN the first event of an input arm waited under the wait bound, THE
/// arm-timing record SHALL carry the wait and SHALL NOT mark it slow, leaving
/// the other measurements alone.
#[test]
fn test_runtime_arm_timing_input_wait_below_bound_is_not_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec
    let slow = timing.record_input_wait(WAIT_WARN_MS - 1, WAIT_WARN_MS);

    // -- Check
    assert!(
        !slow,
        "a wait under {WAIT_WARN_MS} ms must not read as slow"
    );
    assert_eq!(timing.last_input_wait_millis(), WAIT_WARN_MS - 1);
    assert_eq!(timing.last_input_arm_millis(), 0);
    assert_eq!(timing.last_tick_late_millis(), 0);
    Ok(())
}

/// WHEN a wait reaches the bound, THE arm-timing record SHALL mark it slow and
/// keep the wait it was given.
#[test]
fn test_runtime_arm_timing_input_wait_at_and_above_bound_is_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec & Check
    for wait_ms in [WAIT_WARN_MS, WAIT_WARN_MS + 1, 2_000] {
        assert!(
            timing.record_input_wait(wait_ms, WAIT_WARN_MS),
            "a wait of {wait_ms} ms must read as slow"
        );
        assert_eq!(timing.last_input_wait_millis(), wait_ms);
    }
    Ok(())
}

/// WHEN a render tick wakes under the lateness bound, THE arm-timing record
/// SHALL carry the lateness and SHALL NOT mark it late.
#[test]
fn test_runtime_arm_timing_tick_late_below_bound_is_not_slow() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec
    let late = timing.record_tick_late(TICK_LATE_WARN_MS - 1, TICK_LATE_WARN_MS);

    // -- Check
    assert!(
        !late,
        "a tick under {TICK_LATE_WARN_MS} ms late must not read as late"
    );
    assert_eq!(timing.last_tick_late_millis(), TICK_LATE_WARN_MS - 1);
    assert_eq!(timing.last_input_wait_millis(), 0);
    Ok(())
}

/// WHEN a render tick wakes at or past the lateness bound, THE arm-timing
/// record SHALL mark it late and keep the lateness it was given.
#[test]
fn test_runtime_arm_timing_tick_late_at_and_above_bound_is_late() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = ArmTiming::default();

    // -- Exec & Check
    for late_ms in [TICK_LATE_WARN_MS, TICK_LATE_WARN_MS + 1, 5_000] {
        assert!(
            timing.record_tick_late(late_ms, TICK_LATE_WARN_MS),
            "a tick {late_ms} ms late must read as late"
        );
        assert_eq!(timing.last_tick_late_millis(), late_ms);
    }
    Ok(())
}

/// WHEN frames are stamped inside the window, THE fps reading SHALL be their
/// count; WHEN the clock moves past the window, THE old stamps SHALL drop.
#[test]
fn test_runtime_frame_timing_fps_counts_the_rolling_window() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = FrameTiming::default();

    // -- Exec: three frames inside the first second.
    for now_millis in [0u128, 200, 800] {
        timing.record_frame(Duration::from_millis(2), FrameKind::Recomposed, now_millis);
    }

    // -- Check: three painted, three counted.
    assert_eq!(
        timing.fps(900),
        3,
        "every stamped frame is inside the window"
    );
    assert_eq!(
        timing.fps(1_799),
        1,
        "only the frame at 800 ms is within 1000 ms of 1799"
    );

    // -- Exec: the window rolls past the last stamp too.
    let rolled = timing.fps(1_801);

    // -- Check: nothing is left, and the odometer stays put.
    assert_eq!(rolled, 0, "a stamp 1001 ms old leaves the window");
    assert_eq!(
        timing.frames_painted(),
        3,
        "the cumulative counters are untouched by the window"
    );
    Ok(())
}

/// WHEN the surface is idle, THE fps reading SHALL be 0 and SHALL stay 0 as the
/// clock advances: no frame was stamped.
#[test]
fn test_runtime_frame_timing_fps_is_zero_when_idle() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = FrameTiming::default();

    // -- Exec & Check
    for now_millis in [0u128, 500, 10_000] {
        assert_eq!(
            timing.fps(now_millis),
            0,
            "an idle surface paints nothing at {now_millis} ms"
        );
    }
    Ok(())
}

/// WHEN frames paint at a steady cadence, THE fps reading SHALL track the rate
/// under the injected clock — no wall-clock waits.
#[test]
fn test_runtime_frame_timing_fps_tracks_the_cadence() -> Result<()> {
    // -- Setup & Fixtures: the 150 ms spinner cadence, six frames per second.
    let mut timing = FrameTiming::default();

    // -- Exec
    for step in 0..6u128 {
        timing.record_frame(Duration::from_millis(1), FrameKind::ClockCells, step * 150);
    }

    // -- Check
    assert_eq!(
        timing.fps(750),
        6,
        "six frames inside the second read as six"
    );
    assert_eq!(
        timing.fps(1_600),
        2,
        "the stamps at 600 and 750 ms are still within 1000 ms"
    );
    Ok(())
}

/// WHEN a stream arm's phases and folds are recorded, THE breakdown SHALL name
/// every phase, the payload it folded, and the gap between the phase sum and the
/// arm total — the number a mid-arm deschedule hides in.
#[test]
fn test_runtime_stream_arm_timing_breakdown_names_phases_and_gap() -> Result<()> {
    // -- Setup & Fixtures
    let mut timing = StreamArmTiming::default();

    // -- Exec
    timing.reset((0, 0), (0, 0));
    timing.record_text_lane(1);
    timing.record_narration_lane(2);
    timing.record_drain(3);
    timing.record_fold_phase(40);
    timing.record_flush(4);
    timing.record_fold("inference-chunk", 30, 1024);
    timing.record_fold("tool-started", 10, 12);
    let line = timing.format_breakdown(120);

    // -- Check
    for phase in [
        "text=1ms",
        "narr=2ms",
        "drain=3ms",
        "fold=40ms",
        "flush=4ms",
    ] {
        assert!(line.contains(phase), "the line must name {phase}: {line}");
    }
    assert!(
        line.contains("sum=50ms"),
        "the phase sum must be exact: {line}"
    );
    assert!(
        line.contains("arm=120ms"),
        "the arm total must be named: {line}"
    );
    assert!(
        line.contains("gap=-70ms"),
        "the gap must carry the arm's own share, signed: {line}"
    );
    assert!(line.contains("records=2"), "the batch count: {line}");
    assert!(line.contains("bytes=1036"), "the payload byte sum: {line}");
    assert!(
        line.contains("inference-chunk=1/30ms"),
        "per-kind count and total: {line}"
    );
    assert!(
        line.contains("tool-started=1/10ms"),
        "per-kind count and total: {line}"
    );
    Ok(())
}
