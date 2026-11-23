//! The record and control fold: what each bus datum does to the surface, and
//! the notice rows and permission card that come out of it.

use crate::state::{AppState, HISTORY_SOURCE_LABEL, PromptFacts, UiPhase};
use agnt_render::tool_view::ToolView;
use agnt_render::transcript::entry::{NoticeEntry, TranscriptEntry};
use agnt_render::transcript::ir::NoticeKind;
use agnt_runtime::bus::{ControlCommand, LifecycleEvent};
use agnt_runtime::protocol::events::{
    AutoResolution, CompactionRecord, CompactionSource, InferenceRecord, NarrationEvent,
    NoticeKind as RecordNotice, PermissionRecord, PromptStamps, SessionLifecycle, SessionRecord,
    ToolCall, ToolCallId, ToolRecord, TurnRecord,
};
use agnt_runtime::tools::handler::ToolSource;

use super::RenderInput;
use super::keys::reduce_permission_decided;

pub(crate) const PERMISSION_TIMEOUT_STATUS: &str = "permission request timed out — denied";

pub(super) fn reduce_input(state: &mut AppState, input: RenderInput<'_>) -> bool {
    match input {
        RenderInput::Control(ControlCommand::PermissionDecided { decision, .. }) => {
            reduce_permission_decided(state, *decision)
        }
        // A panel request: the surface owns what a panel looks like, so it
        // queues the action and applies it when the fold returns.
        RenderInput::Control(ControlCommand::SharedUiAction { action }) => {
            state.queue_launch_request(*action);
            true
        }
        // A prompt record is a row, never a submission: the loop dispatches
        // every turn from its own queue, so a peer's row arrives after the
        // dispatch, and a replayed row is history that the replay lane places.
        // The surface submits only through its own key lanes; a record that
        // reached the submit queue would run the prompt as a new user turn,
        // unattributed, behind the turn that already owns it.
        RenderInput::Record(SessionRecord::Prompt(record)) => {
            state.push_user_row(record.text.clone(), prompt_facts(&record.stamps));
            true
        }
        RenderInput::Record(SessionRecord::Inference(InferenceRecord::Started { inference })) => {
            state.llm.started(
                &mut state.transcript,
                &mut state.status,
                &mut state.phase,
                &mut state.input_locked,
                *inference,
            );
            state.ensure_invariants();
            true
        }
        RenderInput::Narration(NarrationEvent::InferenceChunk {
            inference,
            aggregated,
            ..
        }) => state.llm.chunk(
            &mut state.transcript,
            &mut state.scroll,
            *inference,
            aggregated,
        ),
        // A record chunk carries the same aggregated text a narrated one does:
        // the replay path publishes chunks on the stream, and both vocabularies
        // fold to one streamed block.
        RenderInput::Record(SessionRecord::Inference(InferenceRecord::Chunk {
            inference,
            aggregated,
            ..
        })) => state.llm.chunk(
            &mut state.transcript,
            &mut state.scroll,
            *inference,
            aggregated,
        ),
        RenderInput::Record(SessionRecord::Inference(InferenceRecord::Completed { usage })) => {
            state.llm.completed(&mut state.status, usage)
        }
        RenderInput::Record(SessionRecord::Inference(InferenceRecord::Stopped {
            cause,
            reason,
            ..
        })) => state.llm.stopped(
            &mut state.transcript,
            &mut state.status,
            &mut state.scroll,
            reason,
            *cause,
        ),
        RenderInput::Record(SessionRecord::Tool(ToolRecord::Started { call })) => {
            state.tool.start_call(&mut state.transcript, call);
            // A call the replayed log carries has no gate record: while the
            // replay is folding, the drawer derives its card from the call
            // itself. A live call derives nothing here — its card comes from
            // the gate record.
            state
                .tool
                .derive_replayed_preview(&mut state.transcript, call);
            true
        }
        RenderInput::Record(SessionRecord::Tool(ToolRecord::Completed { call, outcome })) => state
            .tool
            .complete_call(&mut state.transcript, call, outcome),
        RenderInput::Record(SessionRecord::Permission(
            record @ PermissionRecord::Requested { request_id, .. },
        )) => {
            apply_permission_request_display(state, record);
            state.permission.open_prompt(request_id.as_str());
            state.scroll.scroll_transcript_to_bottom();
            state.ensure_invariants();
            true
        }
        RenderInput::Record(SessionRecord::Permission(PermissionRecord::TimedOut {
            request_id,
        })) => {
            let changed = state.permission.close_prompt_if(request_id.as_str());
            if changed {
                state.status.message.set_message(PERMISSION_TIMEOUT_STATUS);
            }
            changed
        }
        // The echo records of the permission lifecycle are not rendered by the
        // TUI: the card is the only surface, and it closes on submission. A
        // policy resolution is the one that still owes the call its card: the
        // policy decided before any card was opened, so the record is the only
        // carrier of the card's trigger.
        RenderInput::Record(SessionRecord::Permission(
            PermissionRecord::Decided { .. }
            | PermissionRecord::Ignored { .. }
            | PermissionRecord::Cancelled { .. },
        )) => false,
        RenderInput::Record(SessionRecord::Permission(
            record @ PermissionRecord::AutoResolved { .. },
        )) => apply_auto_resolved_display(state, record),
        RenderInput::Narration(NarrationEvent::SystemMessage { kind, text }) => match kind {
            RecordNotice::Warning => {
                state.status.message.set_warning(text.clone());
                true
            }
            RecordNotice::TurnError => handle_turn_error(state, text),
        },
        // A notice record is a persisted row: it renders as the transcript line
        // a live one showed, not as the transient status message.
        RenderInput::Record(SessionRecord::Notice(record)) => match record.kind {
            RecordNotice::Warning => {
                push_system_notice(state, record.text.clone());
                true
            }
            RecordNotice::TurnError => handle_turn_error(state, &record.text),
        },
        RenderInput::Record(SessionRecord::Compaction(CompactionRecord::Started { source })) => {
            state
                .compaction
                .started(&mut state.transcript, source_label(Some(*source)))
        }
        RenderInput::Record(SessionRecord::Compaction(CompactionRecord::SummaryChunk {
            source,
            aggregated,
            ..
        })) => state.compaction.summary_chunk(
            &mut state.transcript,
            &mut state.scroll,
            source_label(Some(*source)),
            aggregated,
        ),
        RenderInput::Record(SessionRecord::Compaction(CompactionRecord::Completed {
            source,
            summary,
        })) => state.compaction.completed(
            &mut state.transcript,
            &mut state.status,
            source_label(*source),
            summary,
        ),
        RenderInput::Record(SessionRecord::Compaction(CompactionRecord::Failed {
            source,
            message,
        })) => state.compaction.failed(
            &mut state.transcript,
            &mut state.status,
            source_label(Some(*source)),
            message,
        ),
        RenderInput::Record(SessionRecord::Compaction(CompactionRecord::Uncompacted {
            source,
        })) => state.compaction.uncompacted(
            &mut state.transcript,
            &mut state.status,
            source_label(Some(*source)),
        ),
        // The spine is the turn end's single observer: the executor publishes
        // this step on a successful turn and on a cancelled one, and the spine
        // never drops a step — so a lagged stream ring cannot strand the input
        // lock. Its ring record is display-only and releases nothing.
        RenderInput::Lifecycle(LifecycleEvent::TurnCompleted { .. }) => release_turn(state),
        // The turn end's ring record is display-only for a live turn: the
        // stream ring is lossy, so it is never the observer of a release, and a
        // record that outlives a lag or folds after the next turn started
        // carries no state change. A replay batch is the exception: the switch
        // publishes the switched-to session's whole log through this fold, and
        // that log ends every turn it closed at rest, exactly as the startup
        // lane's boundary does. The latch closes on the first live turn's
        // start, so a live record still folds as a no-op.
        RenderInput::Record(SessionRecord::Turn(TurnRecord::Completed { .. })) => {
            if state.tool.replay_open() {
                release_turn(state)
            } else {
                false
            }
        }
        // A session boundary. A fresh start is a new transcript: `/new` mints
        // the session the surface now shows, so the rows of the session that
        // ended leave the screen and the startup banner takes their place. The
        // display does not cross the boundary either: a fresh start settles the
        // departed session exactly as a switch does. A hydrated start is a
        // resume — the history it loaded arrived with it, so the fold keeps every
        // row it has and pushes nothing of its own.
        RenderInput::Record(SessionRecord::Session(SessionLifecycle::Started {
            hydrated: false,
            ..
        })) => {
            state.clear_transcript();
            state.push_startup_logo();
            settle_previous_session(state);
            true
        }
        RenderInput::Record(SessionRecord::Session(SessionLifecycle::Started {
            hydrated: true,
            ..
        })) => false,
        // A switch moves the surface to another session: the rows of the
        // session that left go, the banner marks the new one, and the replay
        // that the switch publishes next appends the loaded history below it.
        // The replay folds here and ends at the first live turn's start.
        //
        // The display does not cross the boundary: `phase`, the input lock, the
        // open card, the compaction display, and the status line are the
        // surface's, not a session's, so the session that left can leave them
        // set. A cycle still owning the surface is a live turn and keeps its
        // state.
        RenderInput::Record(SessionRecord::Session(SessionLifecycle::Switched { .. })) => {
            state.clear_transcript();
            state.push_startup_logo();
            state.tool.open_replay();
            settle_previous_session(state);
            true
        }
        // The first live turn after a replay batch ends it: from here the gate
        // record is the only source of a card, exactly as it is for any live
        // turn. Every other lifecycle step folds as a no-op.
        RenderInput::Lifecycle(LifecycleEvent::TurnStarted { .. }) => {
            state.tool.close_replay();
            false
        }
        // The remaining vocabularies: the rest of the turn spine, the
        // narration markers the drawer has no row for, the records with no row,
        // and the control commands addressed to another owner. A compaction
        // request and the loop's own prompt intake are the loop's business,
        // never the surface's.
        RenderInput::Lifecycle(_)
        | RenderInput::Narration(_)
        | RenderInput::Record(_)
        | RenderInput::Control(_) => false,
    }
}

/// The label this drawer puts on a compaction's source. A record that names no
/// source is a replayed one, and the drawer names that row itself.
fn source_label(source: Option<CompactionSource>) -> &'static str {
    match source {
        Some(CompactionSource::Auto) => "auto",
        Some(CompactionSource::Manual) => "slash",
        None => HISTORY_SOURCE_LABEL,
    }
}

/// The footer facts a prompt record carries, as the surface's own type.
///
/// The record's stamps are the facts the accepting boundary knew; the row the
/// record folds carries exactly these, never a re-derivation of the instant.
pub(crate) fn prompt_facts(stamps: &PromptStamps) -> PromptFacts {
    PromptFacts {
        millis: stamps.millis,
        model: stamps.model.clone(),
        agent: stamps.agent.clone(),
    }
}

/// The turn-end release: complete the turn's cycle, close a surviving card,
/// and close the turn's transcript block.
///
/// The live turn's spine completion owns it. The replay boundary owns it too:
/// a replayed log ends every turn it closed at rest, exactly as the live turn
/// did, so both call sites run this one sequence.
pub(crate) fn release_turn(state: &mut AppState) -> bool {
    state.finalize_cycle();
    // A cancel can end the turn while a card is still up (A2A task cancel,
    // Ctrl-C quit+cancel). The request is gone by then, so the card must not
    // outlive the turn.
    state.permission.close_prompt();
    state.turn.finalize(
        &mut state.transcript,
        &mut state.status,
        &mut state.input_locked,
    )
}

/// Settle the display the session that left was showing.
///
/// `phase`, the input lock, the open permission card, the compaction display,
/// and the status line belong to the surface, not to a session, so the session
/// that left can leave them set and the session that arrived would inherit them:
/// a card that swallows every key, a spinner for a sweep whose rows the
/// boundary's transcript clear dropped, and a marker with no lifetime of its own
/// (`StatusMessageKind::Notice` has no TTL, so nothing but the next turn start
/// retires it — and a replay of an empty history starts no turn). A prompt cycle
/// still owning the surface is a live turn and keeps its state; with none, the
/// display state is dropped and the invariant re-derives the rest.
///
/// Both boundaries run it: a switch, whose replay restores the arriving history
/// below the banner, and a fresh start, whose session has none to restore. The
/// fold sits beside the live turn's own release: both write the event-fold state,
/// and the reducer is its single writer.
pub(crate) fn settle_previous_session(state: &mut AppState) {
    let live = state.active_prompt_id.is_some() || !state.pending_prompt_ids.is_empty();
    if !live {
        state.phase = UiPhase::Idle;
        state.input_locked = false;
        // The card answers a request the session that left owned: the resolver
        // dropped that request with its session, so the arriving surface must
        // not hold a card it can only dismiss.
        state.permission.close_prompt();
        state.compaction.clear();
        // The status line is the departing session's report to a surface no
        // cycle owns, so it does not cross either.
        state.status.message.clear();
    }
    state.ensure_invariants();
}

/// A turn-level error is shown as a system notice in the transcript and closes
/// the turn, exactly as the former `TurnError` arm did.
fn handle_turn_error(state: &mut AppState, message: &str) -> bool {
    push_system_notice(state, format!("Error: {message}"));
    release_turn(state)
}

/// Push a system line into the transcript. Both the live error path and the
/// replayed snapshot path funnel through here, so a resumed session shows the
/// same notice rows a live one does.
pub(crate) fn push_system_notice(state: &mut AppState, text: String) {
    state
        .transcript
        .push_entry(TranscriptEntry::Notice(NoticeEntry {
            kind: NoticeKind::System,
            text,
        }));
}

/// Attach a permission request's preview to the pending call it refers to. The
/// request carries the exact call key — `{name}\n{arguments}` — so the card is
/// derived from the same two facts the live path uses; a decorated display name
/// never equals a key and is never matched.
pub(crate) fn apply_permission_request_display(state: &mut AppState, record: &PermissionRecord) {
    let PermissionRecord::Requested {
        call_id,
        shows_preview,
        preview,
        ..
    } = record
    else {
        return;
    };
    attach_call_card(state, call_id, *shows_preview, preview.as_deref());
}

/// Attach the card a policy resolution owes the call it allowed.
///
/// The policy decided before any card was opened, so this record is the only
/// carrier of the card's trigger — but it carries the same two facts the gate
/// record carries: the exact call key and the content only the producer can
/// supply. The drawer derives the rest, so both folds paint one card for one
/// call. A denial, a call with no card, and a record naming no call paint
/// nothing.
fn apply_auto_resolved_display(state: &mut AppState, record: &PermissionRecord) -> bool {
    let PermissionRecord::AutoResolved {
        action,
        call_id,
        shows_preview,
        preview,
        ..
    } = record
    else {
        return false;
    };
    if *action != AutoResolution::Allow {
        return false;
    }
    let Some(call_id) = call_id else {
        return false;
    };
    attach_call_card(state, call_id, *shows_preview, preview.as_deref())
}

/// Derive a call's pre-run card and attach it to the pending call named by its
/// key. The drawer needs the call's identity and the producer's content, never
/// a pre-rendered payload: the key is split back into the parts a call is made
/// of and the card is derived from them, exactly as the live path derives it.
///
/// Returns whether a card was set: a call with no card and a call nothing is
/// tracking under the key both leave the surface unchanged.
fn attach_call_card(
    state: &mut AppState,
    call_id: &ToolCallId,
    shows_preview: bool,
    preview: Option<&str>,
) -> bool {
    // The drawer derives the card from the call itself; the producer supplies
    // content only when it computed one, and the flag says whether a card
    // exists at all.
    if !shows_preview {
        return false;
    }
    let key = call_id.as_str();
    let (name, arguments) = key.split_once('\n').unwrap_or((key, ""));
    let call = ToolCall::new(name, ToolSource::Unknown, arguments);
    let Some(display) = call.preview_body(preview) else {
        return false;
    };
    state
        .tool
        .set_preview_for_key(&mut state.transcript, key, &display)
}
