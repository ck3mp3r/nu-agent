use super::journal::CachedMemory;
use crate::types::Message;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A `CachedMemory` whose rig-memory store is in memory.
fn test_memory() -> CachedMemory {
    CachedMemory::new(crate::session::RigStoreConfig::in_memory())
}

/// Assert that two messages are structurally equal.
fn assert_msg_eq(left: &Message, right: &Message) {
    assert_eq!(
        serde_json::to_value(left).expect("message should serialize"),
        serde_json::to_value(right).expect("message should serialize"),
    );
}

/// Assert that two message lists are structurally equal.
fn assert_msgs_eq(left: &[Message], right: &[Message]) {
    assert_eq!(left.len(), right.len(), "message count mismatch");
    for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
        assert_eq!(
            serde_json::to_value(l).expect("message should serialize"),
            serde_json::to_value(r).expect("message should serialize"),
            "message {i} mismatch",
        );
    }
}

// region:    --- Modules

#[path = "journal/bound.rs"]
mod bound;

#[path = "journal/load.rs"]
mod load;

// endregion: --- Modules
