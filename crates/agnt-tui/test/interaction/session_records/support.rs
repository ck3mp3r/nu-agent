//! Shared support for the session-lifecycle fold tests.
//!
//! The record builders, the shared imports, and the `Result` alias live here so
//! each topical file (`started`, `switched`, `clear`) stays a thin battery over
//! one record family.

pub(super) use crate::state::AppState;
pub(super) use crate::test_support::{
    entry_text, fold_record, prompt_record, tool_completed, tool_started,
};
pub(super) use agnt_render::transcript::entry::{BannerEntry, TranscriptEntry};
pub(super) use agnt_render::transcript::renderer::ItemStatus;
pub(super) use agnt_runtime::protocol::events::{
    InferenceRecord, SessionRecord, ToolCall, ToolName, ToolOutcome, Usage,
};
pub(super) use agnt_runtime::session::tool_status::ToolStatus;
pub(super) use agnt_runtime::tools::handler::ToolSource;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One `edit` call. The same name and arguments give the same call key.
pub(super) fn edit_call() -> ToolCall {
    ToolCall::new(
        ToolName("edit".to_string()),
        ToolSource::Builtin,
        r#"{"path":"a.rs"}"#,
    )
}

/// The completion an `edit` reports: the diff it made over the path it names.
pub(super) fn edit_outcome() -> ToolOutcome {
    ToolOutcome {
        status: ToolStatus::Success,
        payload: None,
        text: r#"{"path":"a.rs","diff":"@@ -1 +1 @@\n--- a\n+++ b\n"}"#.to_string(),
        message: None,
        kind: None,
    }
}

/// Push one entry past the store cap, so the session has an eviction history
/// when the clear runs. The cap is 2000 entries.
pub(super) fn push_filler_past_cap(state: &mut AppState) {
    for _ in 0..FILLERS_PAST_CAP {
        state
            .transcript
            .push_entry(TranscriptEntry::Banner(BannerEntry {
                text: "filler".to_string(),
            }));
    }
}

pub(super) const FILLERS_PAST_CAP: usize = 2001;

pub(super) use crate::test_support::{
    session_agent_switched, session_ended, session_model_switched, session_switched,
};

/// The record the runtime publishes when it mints a session, keyed by whether
/// the start loaded history.
pub(super) fn session_started(hydrated: bool) -> SessionRecord {
    if hydrated {
        crate::test_support::session_started_resumed()
    } else {
        crate::test_support::session_started_new()
    }
}
