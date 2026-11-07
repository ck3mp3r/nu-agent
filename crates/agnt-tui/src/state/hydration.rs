//! Hydration = event replay: the snapshot projection and the replay driver.
//!
//! A persisted session is projected onto the same [`SystemEvent`] log a live
//! turn produces, then folded through the same [`reduce`]. There is no second
//! hydration path: the transcript-path state a resumed session shows (entries,
//! status tokens, compaction bookkeeping) is the fold of the projected log, so
//! a replay and a live reduction of the same facts cannot disagree.
//!
//! The rig conversation store stays the persistence and
//! [`UiMessageSnapshot`] stays the transport. The projection is a pure
//! function of the snapshot list; the driver only folds and discards the
//! replayed effects.

use agnt_render::transcript::ir::ToolName;
use agnt_runtime::protocol::contracts::{UiMessageSnapshot, UiMessageUsageSnapshot};
use agnt_runtime::protocol::events::{SystemEvent, ToolCall, ToolOutcome, Usage};
use agnt_runtime::session::tool_status::ToolStatus;

use super::AppState;
use crate::interaction::cancel::CancelController;
use crate::interaction::reducer::{push_system_notice, reduce};

// region:    --- Types

/// One step of the replayed log: an event the reducer folds, or a persisted
/// system line that names no event of its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ReplayStep {
    Event(Box<SystemEvent>),
    Notice(String),
}

impl ReplayStep {
    /// An event step. Boxed because a `SystemEvent` payload dwarfs the notice
    /// string, and the log is built once per hydration.
    pub(crate) fn event(event: SystemEvent) -> Self {
        Self::Event(Box::new(event))
    }
}

// endregion: --- Types

// region:    --- Public

/// Restore a session's transcript-path state by replaying the persisted
/// snapshots through the reducer.
///
/// Every event folds through [`reduce`]; the outbound effects the replayed log
/// produces are drained and discarded — hydration must not re-execute the
/// session. The drain is still observable: its prompt submission is what
/// places the replayed user rows in the transcript.
///
/// `last_total_tokens` carries the session's displayed total, which the
/// persisted log does not itself carry; it is applied after the fold, exactly
/// where the live status line would have left it.
pub(crate) fn hydrate_by_replay(
    state: &mut AppState,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    last_total_tokens: Option<u64>,
) {
    let cancel_controller = CancelController::default();
    for step in project_snapshots(messages) {
        match step {
            ReplayStep::Event(event) => {
                reduce(state, *event, None);
                // Effects out, never in: the replayed log's outbound effects are
                // not re-executed. The drain consumes the queue and its result
                // is dropped.
                let _ = state.take_pending_events(&cancel_controller);
                let _ = state.take_pending_ui_state_events();
            }
            ReplayStep::Notice(text) => {
                push_system_notice(state, text);
            }
        }
    }
    if let Some(tokens) = last_total_tokens {
        state.status.tokens.hydrate_latest_total_tokens(tokens);
    }
}

/// Project a persisted snapshot list onto the event log it represents.
///
/// One assistant snapshot becomes one streamed chunk carrying the whole text in
/// both fields (a non-streaming producer sends the full text) plus its
/// completion. A tool snapshot becomes the started call, the gate preview the
/// display snapshot after it carries, and the displayless completion — the
/// three-event route the live gate flow uses, so a replayed display lands in the
/// same preview slot a live one does. A compaction snapshot becomes one
/// completion. A role outside the resolver's closed set is a persisted system
/// line: it replays as a system notice, the same row the pre-replay hydration
/// pushed for it.
pub(crate) fn project_snapshots(
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
) -> Vec<ReplayStep> {
    let messages: Vec<UiMessageSnapshot> = messages.into_iter().collect();
    let mut steps = Vec::new();
    let mut turn_open = false;

    for message in &messages {
        match message.role() {
            "user" => {
                if message.content().trim().is_empty() {
                    continue;
                }
                if turn_open {
                    push_turn_boundary(&mut steps);
                }
                steps.push(ReplayStep::event(SystemEvent::PromptSubmitted {
                    text: message.content().to_string(),
                    // The snapshot carries the facts the join recorded; the
                    // replay passes them on verbatim, never filling one in.
                    stamp_millis: message.stamp_millis(),
                    stamp_model: message.stamp_model().map(str::to_string),
                    stamp_agent: message.stamp_agent().map(str::to_string),
                }));
                turn_open = true;
            }
            "assistant" => {
                let text = message.content();
                if text.trim().is_empty() {
                    continue;
                }
                steps.push(ReplayStep::event(SystemEvent::InferenceStarted));
                steps.push(ReplayStep::event(SystemEvent::InferenceChunk {
                    delta: text.to_string(),
                    aggregated: text.to_string(),
                }));
                steps.push(ReplayStep::event(SystemEvent::InferenceCompleted {
                    usage: usage_of(text, message.usage()),
                }));
                turn_open = true;
            }
            "tool" => {
                let Some((call, outcome)) = tool_pair(message) else {
                    continue;
                };
                steps.push(ReplayStep::event(SystemEvent::ToolStarted {
                    call: call.clone(),
                }));
                // A replayed row carries no gate preview: the diff the gate
                // computed was never persisted. The surface derives whatever the
                // call alone supports, so the event carries no content.
                steps.push(ReplayStep::event(SystemEvent::PreviewRequested {
                    call: call.clone(),
                    preview: None,
                }));
                steps.push(ReplayStep::event(SystemEvent::ToolCompleted {
                    call,
                    outcome,
                }));
                turn_open = true;
            }
            // A persisted display row names no call and no transcript fact: the
            // surface derives every display from the call it belongs to.
            "tool_display" => {}
            "compaction" => {
                let body = message.content().to_string();
                steps.push(ReplayStep::event(SystemEvent::CompactionCompleted {
                    source: "history".to_string(),
                    summary_preview: body.clone(),
                    summary_body: body,
                }));
                turn_open = true;
            }
            // A system line outside the resolver's closed set: the pre-replay
            // hydration showed it as a system notice, so the replay does too.
            // An empty line carries no fact.
            _ => {
                let text = message.content().trim();
                if !text.is_empty() {
                    steps.push(ReplayStep::Notice(text.to_string()));
                }
            }
        }
    }

    if turn_open {
        push_turn_boundary(&mut steps);
    }
    steps
}

// endregion: --- Public

// region:    --- Support

/// The turn-at-rest marker, pushed at every turn boundary.
///
/// The rig store persists messages, not turn boundaries: a user snapshot
/// starts a new turn, so every turn before it is complete. The reducer's
/// `TurnCompleted` arm restores the idle baseline (phase, input lock, streaming
/// cursors); its payload carries no information — a live log's payload is
/// ignored by the same arm.
fn push_turn_boundary(steps: &mut Vec<ReplayStep>) {
    steps.push(ReplayStep::event(SystemEvent::TurnCompleted {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: false,
        duration_ms: 0,
    }));
}

/// The usage one assistant snapshot reports. A snapshot that carries no usage
/// reports none of its token counters — the reducer records what the wire would
/// have reported.
fn usage_of(content: &str, persisted: Option<UiMessageUsageSnapshot>) -> Usage {
    Usage {
        response_chars: content.chars().count(),
        tool_calls: 0,
        input_tokens: persisted
            .and_then(|usage| usage.input_tokens())
            .unwrap_or(0),
        output_tokens: persisted
            .and_then(|usage| usage.output_tokens())
            .unwrap_or(0),
        total_tokens: persisted.and_then(|usage| usage.total_tokens()),
        cost: None,
        cost_complete: false,
    }
}

/// The replayed call one tool snapshot describes: the call, the display that
/// docks in its PREVIEW slot, and its displayless completion outcome.
///
/// The snapshot carries the call and its result as data — the same two facts a
/// live turn narrates — so the surface derives the display exactly as it does
/// live. The outcome carries the status semantics only: the persisted verdict,
/// and the not-run kind the reducer classifies as a cancelled call.
///
/// A snapshot without a name or without arguments names no call: the persisted
/// status line is no longer parsed, so such a row is dropped.
fn tool_pair(message: &UiMessageSnapshot) -> Option<(ToolCall, ToolOutcome)> {
    let name = message.tool_name()?;
    let arguments = message.tool_arguments()?;
    let call = ToolCall::new(
        ToolName(name.to_string()),
        agnt_runtime::tools::handler::ToolSource::Unknown,
        arguments,
    );
    let outcome = ToolOutcome {
        // A row with no persisted verdict was not a success. The removed
        // `Unknown` status was hydration-only; a completion carries a verdict.
        success: message.tool_success().unwrap_or(false),
        // The status the resolver recorded for the row. A snapshot carrying
        // none names no status; the replay rule makes such a row a failed
        // completion, the same rule the resolver applies.
        status: message.tool_status().unwrap_or(ToolStatus::Failed),
        result: message.content().trim().to_string(),
        error_kind: None,
        message: None,
    };
    Some((call, outcome))
}

// endregion: --- Support
