//! The ARCH-6 rows recorded for `agnt-storage`.

pub const ROWS: &[&str] = &[
    "ARCH-6 agnt-storage/src/engine.rs:45: `Work` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/error.rs:13: `Result` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/identity.rs:25: `TranscriptId` has no consumer outside `agnt-storage`",
];

/// The impl-member rows: a `pub` member on a type this crate surfaces, with no
/// consumer outside it.
pub const MEMBER_ROWS: &[&str] = &[
    "ARCH-6 agnt-storage/src/rig_adapter/mod.rs:45: `new` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/rig_adapter/mod.rs:66: `session_ids` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/rig_adapter/store.rs:34: `transcript` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/entries.rs:53: `new` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/entries.rs:84: `record` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/entries.rs:105: `read` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/settle.rs:48: `new` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/settle.rs:64: `record` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/tool_calls.rs:55: `new` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/tool_calls.rs:71: `record` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/tool_calls.rs:84: `read` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/transcript.rs:37: `record_transcript` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/turns.rs:49: `new` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/turns.rs:64: `record` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/services/turns.rs:76: `read` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/storage.rs:42: `open` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/storage.rs:52: `in_memory` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/storage.rs:78: `call` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/storage.rs:92: `call_blocking` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/identity.rs:39: `id` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/identity.rs:123: `as_str` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:147: `new` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:161: `with_turn_seq` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:173: `session` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:178: `row_seq` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:183: `turn_seq` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:188: `row` has no consumer outside `agnt-storage`",
];
