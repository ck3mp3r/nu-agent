//! The coordinator's instrumentation: the frame, arm, wait, and tick readings the
//! HUD and the warn lines consume, and the stream arm's phase split.

use std::time::Instant;

use agnt_render::transcript::renderer::ItemStatus;

use super::RuntimeCoordinator;
use crate::runtime::frame_timing::{ArmTiming, FrameTiming, StreamArmTiming};

impl RuntimeCoordinator {
    /// Draft re-wraps the input path has paid for since construction.
    pub(crate) fn layout_recomputes(&self) -> u64 {
        self.layout_recomputes
    }

    pub(crate) fn has_active_animation(&self) -> bool {
        crate::runtime::status::model_activity_label(&self.state) == "busy"
            || self.state.transcript.entries().iter().any(|entry| {
                matches!(
                    entry,
                    agnt_render::transcript::entry::TranscriptEntry::Tool(tool)
                        if tool.status == ItemStatus::InProgress
                )
            })
    }

    /// Whether the busy spinner advanced to a new cadence step since the last
    /// animation frame — the only reason the render loop renders without an
    /// event. The spinner's own 150 ms step is the animation cadence, so a
    /// repeated step reports `false` and the loop emits no bytes: an idle loop
    /// and a stalled animation both stay silent.
    ///
    /// `now_millis` is the frame clock, passed in so the gate is testable
    /// without a wall-clock wait.
    pub(crate) fn spinner_cadence_advanced(&mut self, now_millis: u128) -> bool {
        let step = self
            .has_active_animation()
            .then_some(now_millis / crate::runtime::SPINNER_CADENCE_MS);
        if step == self.last_spinner_step {
            return false;
        }
        self.last_spinner_step = step;
        step.is_some()
    }

    pub(crate) fn expire_status_message_if_due(&mut self, now: Instant) -> bool {
        let abort_expired = self.state.sweep_expired_abort(now);
        if self.state.status.message.expired(now) {
            self.state.status.message.clear();
            return true;
        }
        abort_expired
    }

    pub(crate) fn has_pending_status_message(&self) -> bool {
        self.state.status.message.is_pending()
    }

    /// The loop's painting cost so far: painted frames, the last frame's
    /// duration, and the count of frames that crossed one spinner cadence.
    pub(crate) fn frame_timing(&self) -> &FrameTiming {
        &self.frame_timing
    }

    /// Frames painted within the last second of the frame clock: the reading the
    /// HUD shows. The cumulative counters stay for the log side and `slow`.
    pub(crate) fn fps(&mut self) -> u64 {
        self.frame_timing.fps(self.frame_clock_millis)
    }

    /// The loop's fold-arm durations: the terminal arm's and the stream arm's
    /// last elapsed value, each classified against the warn bound its caller
    /// passes.
    pub(crate) fn arm_timing(&self) -> &ArmTiming {
        &self.arm_timing
    }

    /// Record one completed input-arm fold. The loop passes the elapsed value
    /// its warn check reads and the same bound, so the recorded duration and
    /// the warn line can never disagree about which arm was slow.
    pub(crate) fn record_input_arm(&mut self, elapsed_ms: u128, slow_warn_ms: u128) -> bool {
        self.arm_timing.record_input_arm(elapsed_ms, slow_warn_ms)
    }

    /// Record one completed stream arm, on the same terms as the input arm.
    pub(crate) fn record_stream_arm(&mut self, elapsed_ms: u128, slow_warn_ms: u128) -> bool {
        self.arm_timing.record_stream_arm(elapsed_ms, slow_warn_ms)
    }

    /// Record how long the last input arm's first event waited for its fold.
    ///
    /// The loop passes the wait it measured and the bound its warn check reads,
    /// so the recorded value and the warn line can never disagree.
    pub(crate) fn record_input_wait(&mut self, wait_ms: u128, warn_ms: u128) -> bool {
        self.arm_timing.record_input_wait(wait_ms, warn_ms)
    }

    /// Record how far past its deadline the last render tick woke, on the same
    /// terms as the input wait.
    pub(crate) fn record_tick_late(&mut self, late_ms: u128, warn_ms: u128) -> bool {
        self.arm_timing.record_tick_late(late_ms, warn_ms)
    }

    /// Begin a stream arm: reset the phase split and snapshot the store's
    /// running totals, so the arm reports only what it accrued.
    pub(crate) fn begin_stream_arm(&mut self) {
        self.stream_arm_timing.reset(
            self.state.transcript.remeasure_cost(),
            self.state.transcript.diff_gate_cost(),
        );
    }

    /// The arm's phase split, for the phases the loop times itself.
    pub(crate) fn stream_arm_timing_mut(&mut self) -> &mut StreamArmTiming {
        &mut self.stream_arm_timing
    }

    /// Close the arm's store deltas and render its one-line breakdown.
    pub(crate) fn stream_arm_breakdown(&mut self, arm_ms: u128) -> String {
        self.stream_arm_timing.close_store(
            self.state.transcript.remeasure_cost(),
            self.state.transcript.diff_gate_cost(),
            self.state.transcript.last_remeasure_index(),
        );
        self.stream_arm_timing.format_breakdown(arm_ms)
    }
}
