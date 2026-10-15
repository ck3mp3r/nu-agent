//! Unit tests for the session store.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "store/adjacency.rs"]
mod adjacency;

#[path = "store/fs_store.rs"]
mod fs_store;

#[path = "store/jsonl_store.rs"]
mod jsonl_store;

#[path = "store/llm_context.rs"]
mod llm_context;

#[path = "store/serde.rs"]
mod serde;

#[path = "store/support.rs"]
mod support;

#[path = "store/title.rs"]
mod title;

#[path = "store/trimming.rs"]
mod trimming;

// The monolith resolved `super::<item>` against `crate::session`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules

/// Compare two messages via their serialized JSON form.
///
/// rig 0.38+ uses `#[serde(flatten)]` on `Text::additional_params`.
/// A round-trip through serde turns `None` into `Some(Object {})`,
/// which breaks `PartialEq` even though the two forms are semantically
/// identical. Serializing first normalizes both sides.
pub(crate) fn assert_msg_eq(left: &Message, right: &Message) {
    assert_eq!(
        serde_json::to_value(left).unwrap(),
        serde_json::to_value(right).unwrap(),
    );
}

pub(crate) fn assert_msgs_eq(left: &[Message], right: &[Message]) {
    assert_eq!(left.len(), right.len(), "message count mismatch");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        assert_eq!(
            serde_json::to_value(l).unwrap(),
            serde_json::to_value(r).unwrap(),
            "message {i} mismatch",
        );
    }
}
