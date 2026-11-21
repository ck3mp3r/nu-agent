//! Frame- and arm-duration instrumentation for the render loop.
//!
//! The instrument measures; it never changes what a frame paints or when an arm
//! runs. Frame cost and fold cost are the dominant latency sources while the
//! surface is busy, so the counters stay exact and free of wall-clock guessing.
//! [`FrameTiming`] counts painted frames of each kind, the last frame's duration,
//! and the slow frames. [`ArmTiming`] carries the terminal arm's and the stream
//! arm's last durations, and reports whether each arm crossed the bound the loop
//! passes.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

// region:    --- Types

/// A frame that takes at least this many milliseconds reads as a stutter: one
/// spinner cadence, so the frame spent a whole animation step painting.
pub(crate) const FRAME_SLOW_WARN_MS: u128 = 150;

/// What a frame repaints. The two kinds are the two ways the loop can produce
/// bytes: a full recomposition of the state, and a repaint of the cells that
/// read the frame clock alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// Every region is composed from the state.
    Recomposed,
    /// Only the cells whose glyph reads the frame clock are painted over the
    /// last painted frame: the state did not change, the clock did.
    ClockCells,
}

/// The rolling window the HUD's fps reading counts over: one flat second.
///
/// Flat, not aligned to the tick: aligning would alias with the 150 ms spinner
/// cadence and read as a spinner-rate report rather than the plain frame rate.
const FPS_WINDOW_MS: u128 = 1000;

/// What the render loop knows about its own painting cost.
#[derive(Debug, Default)]
pub(crate) struct FrameTiming {
    frames_painted: u64,
    recomposed_frames: u64,
    clock_frames: u64,
    last_frame_millis: u128,
    slow_frames: u64,
    /// When each painted frame was stamped, ascending: the HUD's rolling window.
    /// A timestamp is pushed once and popped at most once, so the ring's cost is
    /// O(1) amortized and its length is the frame rate itself.
    frame_times: VecDeque<u128>,
}

impl FrameTiming {
    /// Record one completed frame of `kind`, stamped at `now_millis`.
    ///
    /// Returns whether the frame crossed [`FRAME_SLOW_WARN_MS`]. The caller
    /// owns the warn line; this type carries the measurement alone.
    pub(crate) fn record_frame(
        &mut self,
        elapsed: Duration,
        kind: FrameKind,
        now_millis: u128,
    ) -> bool {
        self.frames_painted = self.frames_painted.saturating_add(1);
        match kind {
            FrameKind::Recomposed => {
                self.recomposed_frames = self.recomposed_frames.saturating_add(1);
            }
            FrameKind::ClockCells => {
                self.clock_frames = self.clock_frames.saturating_add(1);
            }
        }
        self.last_frame_millis = elapsed.as_millis();
        let slow = self.last_frame_millis >= FRAME_SLOW_WARN_MS;
        if slow {
            self.slow_frames = self.slow_frames.saturating_add(1);
        }
        self.frame_times.push_back(now_millis);
        self.prune_frame_times(now_millis);
        slow
    }

    /// Frames painted within the last [`FPS_WINDOW_MS`] of `now_millis`: the
    /// rolling reading the HUD shows. Prunes what fell out of the window first.
    pub(crate) fn fps(&mut self, now_millis: u128) -> u64 {
        self.prune_frame_times(now_millis);
        self.frame_times.len() as u64
    }

    /// Drop the stamps that fell out of the window. The ring is ascending, so a
    /// stamp that is too old ends the prefix of too-old ones.
    fn prune_frame_times(&mut self, now_millis: u128) {
        while self
            .frame_times
            .front()
            .is_some_and(|stamp| now_millis.saturating_sub(*stamp) > FPS_WINDOW_MS)
        {
            self.frame_times.pop_front();
        }
    }

    // region:    --- Accessors

    /// Frames painted since construction.
    pub(crate) fn frames_painted(&self) -> u64 {
        self.frames_painted
    }

    /// Frames composed from the state since construction.
    pub(crate) fn recomposed_frames(&self) -> u64 {
        self.recomposed_frames
    }

    /// Frames that repainted the clock-reading cells alone since construction.
    pub(crate) fn clock_frames(&self) -> u64 {
        self.clock_frames
    }

    /// Duration of the most recently painted frame, in milliseconds.
    pub(crate) fn last_frame_millis(&self) -> u128 {
        self.last_frame_millis
    }

    /// Painted frames that crossed [`FRAME_SLOW_WARN_MS`].
    pub(crate) fn slow_frames(&self) -> u64 {
        self.slow_frames
    }

    // endregion: --- Accessors
}

/// What the render loop knows about the cost of its two fold arms: the terminal
/// arm that folds keys, and the stream arm that folds bus records.
#[derive(Debug, Default)]
pub(crate) struct ArmTiming {
    last_input_arm_millis: u128,
    last_stream_arm_millis: u128,
    last_input_wait_millis: u128,
    last_tick_late_millis: u128,
}

impl ArmTiming {
    /// Record one completed input-arm fold.
    ///
    /// Returns whether the arm crossed `slow_warn_ms`. The caller owns the warn
    /// line; this type carries the arm's last duration alone.
    pub(crate) fn record_input_arm(&mut self, elapsed_ms: u128, slow_warn_ms: u128) -> bool {
        self.last_input_arm_millis = elapsed_ms;
        elapsed_ms >= slow_warn_ms
    }

    /// Record one completed stream arm, on the same terms as the input arm.
    pub(crate) fn record_stream_arm(&mut self, elapsed_ms: u128, slow_warn_ms: u128) -> bool {
        self.last_stream_arm_millis = elapsed_ms;
        elapsed_ms >= slow_warn_ms
    }

    /// Record how long the first terminal event of the last input arm waited
    /// between its read and its fold.
    ///
    /// Returns whether the wait crossed `warn_ms`. A wait measures scheduling:
    /// a fold can be fast every time and the keypress still sits unread.
    pub(crate) fn record_input_wait(&mut self, wait_ms: u128, warn_ms: u128) -> bool {
        self.last_input_wait_millis = wait_ms;
        wait_ms >= warn_ms
    }

    /// Record how far past its deadline the last render tick woke.
    ///
    /// Returns whether the lateness crossed `warn_ms`.
    pub(crate) fn record_tick_late(&mut self, late_ms: u128, warn_ms: u128) -> bool {
        self.last_tick_late_millis = late_ms;
        late_ms >= warn_ms
    }

    // region:    --- Accessors

    /// Duration of the most recent input arm, in milliseconds.
    pub(crate) fn last_input_arm_millis(&self) -> u128 {
        self.last_input_arm_millis
    }

    /// Duration of the most recent stream arm, in milliseconds.
    pub(crate) fn last_stream_arm_millis(&self) -> u128 {
        self.last_stream_arm_millis
    }

    /// Wait of the most recent input arm's first event, in milliseconds.
    pub(crate) fn last_input_wait_millis(&self) -> u128 {
        self.last_input_wait_millis
    }

    /// Lateness of the most recent render tick, in milliseconds.
    pub(crate) fn last_tick_late_millis(&self) -> u128 {
        self.last_tick_late_millis
    }

    // endregion: --- Accessors
}

/// One stream arm's cost, phase by phase, plus the payload it folded.
///
/// The phase split is what the arm total cannot give: an arm that spent 60 ms in
/// its phases and 819 ms in total spent the rest descheduled, and only the
/// difference between the phase sum and the arm total says so.
#[derive(Debug, Default)]
pub(crate) struct StreamArmTiming {
    text_lane_ms: u128,
    narration_lane_ms: u128,
    drain_ms: u128,
    fold_ms: u128,
    flush_ms: u128,
    records: u64,
    bytes: u128,
    folds: BTreeMap<&'static str, (u64, u128)>,
    store_baseline: (u128, u64, u128, u64),
    stale_remeasure: (u128, u64),
    diff_gate: (u128, u64),
    stale_remeasure_index: Option<usize>,
}

impl StreamArmTiming {
    /// Begin an arm: every phase, count, and fold total resets, and the store's
    /// running totals are snapshotted so the arm reports only its own share.
    pub(crate) fn reset(&mut self, stale: (u128, u64), gate: (u128, u64)) {
        *self = Self {
            store_baseline: (stale.0, stale.1, gate.0, gate.1),
            ..Self::default()
        };
    }

    /// Record one phase's elapsed milliseconds.
    pub(crate) fn record_text_lane(&mut self, ms: u128) {
        self.text_lane_ms = ms;
    }

    /// Record the narration lane's elapsed milliseconds.
    pub(crate) fn record_narration_lane(&mut self, ms: u128) {
        self.narration_lane_ms = ms;
    }

    /// Record the bounded stream drain's elapsed milliseconds.
    pub(crate) fn record_drain(&mut self, ms: u128) {
        self.drain_ms = ms;
    }

    /// Record the batch fold's elapsed milliseconds.
    pub(crate) fn record_fold_phase(&mut self, ms: u128) {
        self.fold_ms = ms;
    }

    /// Record the flush's elapsed milliseconds.
    pub(crate) fn record_flush(&mut self, ms: u128) {
        self.flush_ms = ms;
    }

    /// Record one folded item under its kind label, with the bytes it carried.
    pub(crate) fn record_fold(&mut self, label: &'static str, ms: u128, bytes: u128) {
        let slot = self.folds.entry(label).or_insert((0, 0));
        slot.0 = slot.0.saturating_add(1);
        slot.1 = slot.1.saturating_add(ms);
        self.records = self.records.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Close the arm: fold the store's totals into their per-arm deltas, and
    /// name the entry the arm's last measure touched.
    pub(crate) fn close_store(
        &mut self,
        stale: (u128, u64),
        gate: (u128, u64),
        stale_index: Option<usize>,
    ) {
        self.stale_remeasure = (
            stale.0.saturating_sub(self.store_baseline.0),
            stale.1.saturating_sub(self.store_baseline.1),
        );
        self.diff_gate = (
            gate.0.saturating_sub(self.store_baseline.2),
            gate.1.saturating_sub(self.store_baseline.3),
        );
        self.stale_remeasure_index = if self.stale_remeasure.1 > 0 {
            stale_index
        } else {
            None
        };
    }

    /// The sum of the five phases.
    pub(crate) fn phase_sum_ms(&self) -> u128 {
        self.text_lane_ms
            .saturating_add(self.narration_lane_ms)
            .saturating_add(self.drain_ms)
            .saturating_add(self.fold_ms)
            .saturating_add(self.flush_ms)
    }

    /// The one-line breakdown the warn writes: every phase, the payload, the
    /// per-kind fold totals, the store deltas, and the gap between the phases
    /// and the arm.
    ///
    /// A negative gap is time the arm held and no phase accounts for — a
    /// mid-arm deschedule, which no fold step can explain.
    pub(crate) fn format_breakdown(&self, arm_ms: u128) -> String {
        let gap_ms = self.phase_sum_ms() as i128 - arm_ms as i128;
        let folds = if self.folds.is_empty() {
            "none".to_string()
        } else {
            self.folds
                .iter()
                .map(|(label, (count, ms))| format!("{label}={count}/{ms}ms"))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "tui-stream-breakdown: text={}ms narr={}ms drain={}ms fold={}ms flush={}ms sum={}ms \
arm={arm_ms}ms gap={gap_ms}ms records={} bytes={} folds={folds} stale-remeasure={}ms/{}@{} \
diff-gate={}ms/{}",
            self.text_lane_ms,
            self.narration_lane_ms,
            self.drain_ms,
            self.fold_ms,
            self.flush_ms,
            self.phase_sum_ms(),
            self.records,
            self.bytes,
            self.stale_remeasure.0,
            self.stale_remeasure.1,
            self.stale_remeasure_index
                .map(|index| index.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.diff_gate.0,
            self.diff_gate.1,
        )
    }
}

// endregion: --- Types

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/runtime/frame_timing.rs"]
mod frame_timing_test;

// endregion: --- Tests
