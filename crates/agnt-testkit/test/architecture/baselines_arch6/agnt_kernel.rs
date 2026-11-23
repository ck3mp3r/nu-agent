//! The ARCH-6 rows recorded for `agnt-kernel`.

pub const ROWS: &[&str] = &[
    "ARCH-6 agnt-kernel/src/bus/cancel.rs:13: `Cancel` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/channel.rs:64: `Metrics` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/channel.rs:74: `NoMetrics` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/events.rs:244: `WorkerPhase` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/mod.rs:54: `Bus` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/ring.rs:119: `Ring` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/ring.rs:193: `Strict` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/records.rs:566: `TurnOutcome` has no consumer outside `agnt-kernel`",
];

/// The impl-member rows: a `pub` member on a type this crate surfaces, with no
/// consumer outside it.
pub const MEMBER_ROWS: &[&str] = &[
    "ARCH-6 agnt-kernel/src/builtin.rs:84: `of_name` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/cancel.rs:26: `request` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/channel.rs:166: `with_metrics` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/channel.rs:243: `channel_with_metrics` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/channel.rs:313: `channel_with_metrics` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/control.rs:38: `subscriber_count` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/facts.rs:34: `capacity` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/facts.rs:49: `subscriber_count` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/lifecycle.rs:38: `subscriber_count` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/narration.rs:33: `capacity` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/narration.rs:48: `subscriber_count` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/ring.rs:125: `with_capacity` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/ring.rs:152: `subscriber_count` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/ring.rs:221: `subscriber_count` has no consumer outside `agnt-kernel`",
    // The one recorded census exception. The consumption chain is
    // `agnt-tui/src/interactive/render_loop/mod.rs:74` calling `subscribe_text` on
    // a `Stream` it names through `agnt-runtime`'s re-export
    // (`agnt-runtime/src/bus/mod.rs:9`), not through `agnt-kernel` itself, so the
    // member-credit gate — the caller must write a path into the declaring crate
    // — never fires. Blind-spot class: a re-export chain. Recorded permanently,
    // not drained.
    "ARCH-6 agnt-kernel/src/bus/stream.rs:65: `subscribe_text` has no consumer outside `agnt-kernel`",
    "ARCH-6 agnt-kernel/src/bus/stream.rs:70: `subscriber_count` has no consumer outside `agnt-kernel`",
];
