//! The ARCH-6 rows recorded for `agnt-storage`.

pub const ROWS: &[&str] = &[
    "ARCH-6 agnt-storage/src/engine.rs:45: `Work` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/error.rs:13: `Result` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/identity.rs:25: `TranscriptId` has no consumer outside `agnt-storage`",
];

/// The impl-member rows: a `pub` member on a type this crate surfaces, with no
/// consumer outside it.
pub const MEMBER_ROWS: &[&str] = &[
    "ARCH-6 agnt-storage/src/storage.rs:78: `call` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/storage.rs:92: `call_blocking` has no consumer outside `agnt-storage`",
    "ARCH-6 agnt-storage/src/transcript/record.rs:178: `row_seq` has no consumer outside `agnt-storage`",
];
