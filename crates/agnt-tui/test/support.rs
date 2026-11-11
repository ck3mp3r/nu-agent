use std::path::PathBuf;

use agnt_render::tool_view::ToolView;
use agnt_render::transcript::entry::TranscriptEntry;
use agnt_runtime::protocol::events::{
    CompactionRecord, CompactionSource, InferenceRecord, NarrationEvent, NoticeKind, PromptRecord,
    PromptStamps, SessionId, SessionLifecycle, SessionRecord, ToolCall, ToolOutcome, ToolRecord,
    TurnRecord, Usage,
};

use crate::interaction::reducer::{RenderInput, reduce};
use crate::state::{ActivePicker, AppState, CommandPaletteAction, PickerOption, PickerPayload};

/// Fold one stream record through the reducer, exactly as the render loop does.
pub(crate) fn fold_record(state: &mut AppState, record: SessionRecord) -> bool {
    reduce(state, RenderInput::Record(&record), None)
}

/// Fold one narration event through the reducer.
pub(crate) fn fold_narration(state: &mut AppState, event: NarrationEvent) -> bool {
    reduce(state, RenderInput::Narration(&event), None)
}

/// The prompt record a surface submission produces.
pub(crate) fn prompt_record(
    text: impl Into<String>,
    millis: Option<i64>,
    model: Option<String>,
    agent: Option<String>,
) -> SessionRecord {
    SessionRecord::Prompt(PromptRecord {
        text: text.into(),
        stamps: PromptStamps {
            millis,
            model,
            agent,
        },
        task_id: None,
    })
}

/// The prompt record a peer's arrival produces: it carries the task whose turn
/// answers it.
pub(crate) fn peer_prompt_record(
    text: impl Into<String>,
    millis: Option<i64>,
    model: Option<String>,
    agent: Option<String>,
    task_id: &str,
) -> SessionRecord {
    SessionRecord::Prompt(PromptRecord {
        text: text.into(),
        stamps: PromptStamps {
            millis,
            model,
            agent,
        },
        task_id: Some(task_id.to_string()),
    })
}

/// The record that opens an inference call.
pub(crate) fn inference_started() -> SessionRecord {
    SessionRecord::Inference(InferenceRecord::Started)
}

/// The record that closes an inference call with its usage totals.
pub(crate) fn inference_completed(
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: Option<u64>,
    cost: Option<f64>,
    cost_complete: bool,
) -> SessionRecord {
    SessionRecord::Inference(InferenceRecord::Completed {
        usage: Usage {
            input_tokens,
            output_tokens,
            total_tokens,
            cost,
            cost_complete,
        },
    })
}

/// The record that stops an inference call before it completes.
pub(crate) fn inference_stopped(reason: impl Into<String>) -> SessionRecord {
    SessionRecord::Inference(InferenceRecord::Stopped {
        cause: agnt_runtime::protocol::events::StopCause::Other,
        reason: reason.into(),
    })
}

/// One streamed inference delta, narrated.
pub(crate) fn inference_chunk(
    delta: impl Into<String>,
    aggregated: impl Into<String>,
) -> NarrationEvent {
    NarrationEvent::InferenceChunk {
        delta: delta.into(),
        aggregated: aggregated.into(),
    }
}

/// A user-facing message, narrated.
pub(crate) fn system_message(kind: NoticeKind, text: impl Into<String>) -> NarrationEvent {
    NarrationEvent::SystemMessage {
        kind,
        text: text.into(),
    }
}

/// The record that closes a turn.
pub(crate) fn turn_completed(duration_ms: u64) -> SessionRecord {
    SessionRecord::Turn(TurnRecord::Completed { duration_ms })
}

/// The record the runtime publishes when it mints a session with no history
/// loaded: the `/new` mint.
pub(crate) fn session_started_new() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::Started {
        session_id: SessionId::new("sess-0000000000000001"),
        hydrated: false,
    })
}

/// The record the runtime publishes when a session starts with the history it
/// resumed already loaded.
pub(crate) fn session_started_resumed() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::Started {
        session_id: SessionId::new("sess-0000000000000001"),
        hydrated: true,
    })
}

/// The record the runtime publishes when the active session switches.
pub(crate) fn session_switched() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::Switched {
        from: Some(SessionId::new("sess-0000000000000001")),
        to: SessionId::new("sess-0000000000000002"),
    })
}

/// The record the runtime publishes when the active agent switches.
pub(crate) fn session_agent_switched() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::AgentSwitched {
        from: None,
        to: "coder".to_string(),
    })
}

/// The record the runtime publishes when the active model switches.
pub(crate) fn session_model_switched() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::ModelSwitched {
        from: "mock-model".to_string(),
        to: "other-model".to_string(),
    })
}

/// The record the runtime publishes when the session closes.
pub(crate) fn session_ended() -> SessionRecord {
    SessionRecord::Session(SessionLifecycle::Ended {
        session_id: SessionId::new("sess-0000000000000001"),
    })
}

/// The record that opens a tool call.
pub(crate) fn tool_started(call: ToolCall) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Started { call })
}

/// The record that settles a tool call.
pub(crate) fn tool_completed(call: ToolCall, outcome: ToolOutcome) -> SessionRecord {
    SessionRecord::Tool(ToolRecord::Completed { call, outcome })
}

/// The record that opens a compaction sweep.
pub(crate) fn compaction_started(source: CompactionSource) -> SessionRecord {
    SessionRecord::Compaction(CompactionRecord::Started { source })
}

/// One streamed compaction-summary delta.
pub(crate) fn compaction_summary_chunk(
    source: CompactionSource,
    delta: impl Into<String>,
    aggregated: impl Into<String>,
) -> SessionRecord {
    SessionRecord::Compaction(CompactionRecord::SummaryChunk {
        source,
        delta: delta.into(),
        aggregated: aggregated.into(),
    })
}

/// The record that finishes a compaction sweep.
pub(crate) fn compaction_completed(
    source: CompactionSource,
    summary: impl Into<String>,
) -> SessionRecord {
    SessionRecord::Compaction(CompactionRecord::Completed {
        source,
        summary: summary.into(),
    })
}

/// The record that fails a compaction sweep.
pub(crate) fn compaction_failed(
    source: CompactionSource,
    message: impl Into<String>,
) -> SessionRecord {
    SessionRecord::Compaction(CompactionRecord::Failed {
        source,
        message: message.into(),
    })
}

/// Flatten one entry to plain text: the markdown body, the tool call line,
/// the task call line, or the notice/banner text.
pub(crate) fn entry_text(entry: &TranscriptEntry) -> String {
    match entry {
        TranscriptEntry::Message(message) => message.markdown.clone(),
        TranscriptEntry::Tool(tool) => tool.call.call_line(),
        TranscriptEntry::Task(task) => format!("{} · {}", task.name, task.summary),
        TranscriptEntry::Notice(notice) => notice.text.clone(),
        TranscriptEntry::Banner(banner) => banner.text.clone(),
    }
}

pub(crate) fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../agnt-render/src/fixtures/markdown")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read markdown fixture {}: {error}",
            path.display()
        )
    })
}

pub(crate) fn open_command_palette_for_test(state: &mut AppState) {
    state.info_panel = None;
    let entry = state.picker.open(ActivePicker::CommandPalette);
    entry.state.options = CommandPaletteAction::PALETTE_ACTIONS
        .iter()
        .map(|a| PickerOption {
            id: a.label().to_string(),
            display: a.label().to_string(),
            search_text: a.label().to_string(),
            section: None,
            sort_key: Vec::new(),
            payload: PickerPayload::Command(*a),
        })
        .collect();
}
