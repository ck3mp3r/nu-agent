//! Hydration = record replay.
//!
//! A persisted session is projected onto the same [`SessionRecord`] log a live
//! turn produces — the projection is the runtime's one [`project_snapshots`] —
//! then folded through the same reducer a live record takes. There is no second
//! hydration path: the transcript-path state a resumed session shows (entries,
//! status tokens, compaction bookkeeping) is the fold of the projected log, so
//! a replay and a live reduction of the same facts cannot disagree.
//!
//! The store stays the persistence. The projection is a pure function of the
//! snapshot list; the replay only folds and discards the effects.

use agnt_render::tool_view::ToolView;
use agnt_runtime::protocol::contracts::{UiMessageSnapshot, project_snapshots};
use agnt_runtime::protocol::events::{CompactionRecord, SessionRecord, ToolRecord};

use super::{AppState, PromptFacts};
use crate::interaction::cancel::CancelController;
use crate::interaction::reducer::{Fold, RenderInput, prompt_facts, reduce};

// region:    --- Types

/// The source label a rehydrated compaction row carries.
///
/// The store persists the summary, not where the sweep came from, so the replay
/// is what names the row: a rehydrated row reads `history`, a live one reads
/// `auto` or `slash`. The label belongs to the drawer, and this is the drawer's
/// replay path saying which row it is drawing.
const HISTORY_SOURCE_LABEL: &str = "history";

/// A replayed compaction row, folded with the replay label.
struct ReplayedCompaction<'a> {
    summary: &'a str,
}

impl Fold for ReplayedCompaction<'_> {
    fn fold(self, state: &mut AppState, _cancel_controller: Option<&CancelController>) -> bool {
        state.compaction.completed(
            &mut state.transcript,
            &mut state.status,
            HISTORY_SOURCE_LABEL,
            self.summary,
        )
    }
}

/// A replayed prompt row: history, folded through the submit queue's own lane.
///
/// The row's placement belongs to the queue machinery — enqueue here, and the
/// replay drain coalesces and places the row exactly as a live submission
/// would, which is what keeps one block per run and leaves the prompt in the
/// input history. The submit lane's slash handling is deliberately absent: a
/// persisted user row is never a slash command, and no `OrchestratorEvent` the
/// replay drain produces survives the record.
struct ReplayedPrompt<'a> {
    text: &'a str,
    facts: PromptFacts,
}

impl Fold for ReplayedPrompt<'_> {
    fn fold(self, state: &mut AppState, _cancel_controller: Option<&CancelController>) -> bool {
        state.enqueue_prompt_with_facts(self.text.to_string(), self.facts);
        true
    }
}

// endregion: --- Types

// region:    --- Public

/// Restore a session's transcript-path state by replaying the persisted
/// snapshots through the reducer.
///
/// Every record folds through [`reduce`]; the outbound effects the replayed log
/// produces are drained and discarded — hydration must not re-execute the
/// session. The drain is still observable: its prompt submission is what places
/// the replayed user rows in the transcript.
///
/// `last_total_tokens` carries the session's displayed total, which the
/// persisted log does not itself carry. When the caller passes none, the total
/// the projection found is used; the fold then places it exactly where the live
/// status line would have left it.
pub(crate) fn hydrate_by_replay(
    state: &mut AppState,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    last_total_tokens: Option<u64>,
) {
    let (records, projected_total) = project_snapshots(messages);
    replay_records(state, records);
    if let Some(tokens) = last_total_tokens.or(projected_total) {
        state.status.tokens.hydrate_latest_total_tokens(tokens);
    }
}

/// Fold one record log through the reducer.
///
/// The replayed effects are not re-executed: the queue the fold fills is
/// consumed and dropped.
pub(crate) fn replay_records(
    state: &mut AppState,
    records: impl IntoIterator<Item = SessionRecord>,
) {
    let cancel_controller = CancelController::default();
    for record in records {
        match &record {
            // The two records whose replay label the log cannot carry: the
            // drawer's own history row for a compaction, and the prompt row the
            // submit queue places once the replay drain consumes it.
            SessionRecord::Compaction(CompactionRecord::Completed { summary, .. }) => {
                reduce(state, ReplayedCompaction { summary }, None);
            }
            SessionRecord::Prompt(record) => {
                reduce(
                    state,
                    ReplayedPrompt {
                        text: &record.text,
                        facts: prompt_facts(&record.stamps),
                    },
                    None,
                );
            }
            other => {
                reduce(state, RenderInput::Record(other), None);
            }
        }
        // A replayed call carries no gate record, so the drawer derives the card
        // from the call itself: a resumed row shows the card the live gate would
        // have shown, minus the producer-only content.
        if let SessionRecord::Tool(ToolRecord::Started { call }) = &record
            && let Some(display) = call.preview_body(None)
        {
            state
                .tool
                .set_tool_preview(&mut state.transcript, call.id(), &display);
        }
        // Effects out, never in: the replayed log's outbound effects are not
        // re-executed. A prompt submission is the drain's only visible result;
        // a persisted slash command must not re-open its panel now.
        let _ = state.take_pending_events(&cancel_controller);
        while state.take_next_launch_request().is_some() {}
    }
}
