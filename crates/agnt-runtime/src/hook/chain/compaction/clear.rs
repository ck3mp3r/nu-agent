//! The near-window clearing branch: the rule that decides whether clearing is
//! enough, the notes a clearing asks for, and the plan the executor persists.
//!
//! Clearing replaces older tool outputs with a placeholder, so it frees room
//! without a summary call. It only earns its place when it leaves enough room —
//! otherwise the summary path runs instead — and when an output it drops cannot
//! be had again, the model writes notes on it first.

use rig::catalog::ModelSpec;
use rig::completion::CompletionRequest;
use rig::conversation::Conversation;
use rig::message::{AssistantContent, CallId, Message, ToolCall};
use rig::run::RequestPatch;
use rig_memory::{ClearPlan, CompactionPolicy, NearWindow, TrackArgument};

use agnt_kernel::CompactionSource;

use super::CompactionPath;
use super::in_place::budget;
use crate::conversation::compaction::Chunks;

// region:    --- Types

/// How much of `limit - reserve` must be free after a clearing, as a divisor:
/// half. A clearing that frees less changes nothing about what the next request
/// sends, so the summary path takes the turn.
const ENOUGH: u64 = 2;

/// What a clearing has to write: which outputs it drops, and the notes on them
/// when they could not be had again.
///
/// The decision stashes it and the executor takes it: the splice that persists
/// the cleared outputs sits between the two runs, the only window a store write
/// survives. The plan names entries by id, so it still applies after the run's
/// own exchange joined the conversation.
#[derive(Clone)]
pub(crate) struct PendingClearance {
    plan: ClearPlan,
    notes: Option<String>,
}

// endregion: --- Types

// region:    --- Implementations

impl PendingClearance {
    /// The clearing `plan` with the `notes` written on it.
    pub(crate) fn new(plan: ClearPlan, notes: Option<String>) -> Self {
        Self { plan, notes }
    }

    /// The plan that names the outputs to clear.
    pub(crate) fn plan(&self) -> &ClearPlan {
        &self.plan
    }

    /// The notes on the outputs it drops, `None` when every output can be had
    /// again.
    pub(crate) fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }
}

// endregion: --- Implementations

// region:    --- Functions

/// The clearing this turn takes instead of a summary, when it takes one: the
/// patch that spares this call the cleared outputs, the plan the executor
/// persists, and the notes written on them.
///
/// `None` when there is nothing to clear, when clearing frees too little of the
/// room, or when the outputs it would drop cannot be had again and their notes
/// cannot be written — the summary path then keeps the turn.
pub(super) async fn decision(
    conversation: &Conversation,
    tokens: u64,
    spec: &ModelSpec,
    policy: &CompactionPolicy,
    path: &CompactionPath<'_>,
) -> Option<(RequestPatch, ClearPlan, Option<String>)> {
    let plan = policy.clearing.plan(conversation);
    if plan.cleared.results == 0 || !leaves_enough(tokens, &plan, spec, policy) {
        return None;
    }
    let notes = if notes_needed(policy, conversation, &plan) {
        Some(write_notes(policy, spec, conversation, tokens, path).await?)
    } else {
        None
    };
    let patch = patch(conversation, &plan, notes.as_deref());
    Some((patch, plan, notes))
}

/// Whether `plan` frees enough of the room to be taken instead of a summary.
///
/// The rule is rig-basics': the request that would follow the clearing must
/// leave half of `limit - reserve` free. A model whose input limit is unknown
/// never triggers, so there is no room to measure against and no clearing.
pub(super) fn leaves_enough(
    tokens: u64,
    plan: &ClearPlan,
    spec: &ModelSpec,
    policy: &CompactionPolicy,
) -> bool {
    let Some(limit) = spec.input_limit().map(u64::from) else {
        return false;
    };
    let room = limit.saturating_sub(policy.summarizer.limits.reserve);
    let remaining = tokens.saturating_sub(plan.cleared.tokens as u64);
    remaining.saturating_mul(ENOUGH) <= room
}

/// Write the notes on the outputs `plan` drops, `None` when the note request
/// cannot fit or when the model answers nothing usable.
///
/// A request that cannot fit asks for nothing: the room below the model's input
/// limit is smaller than the least an answer is worth.
async fn write_notes(
    policy: &CompactionPolicy,
    spec: &ModelSpec,
    conversation: &Conversation,
    tokens: u64,
    path: &CompactionPath<'_>,
) -> Option<String> {
    let request = notes_request(policy, spec, conversation, tokens)?;
    path.compaction
        .compactor
        .complete(path.model, request, CompactionSource::Auto, Chunks::Silent)
        .await
        .ok()
}

/// The request this call sends: `conversation` with `plan`'s outputs cleared and
/// `notes` written after them, when there are any.
///
/// The turn's own history is untouched: the executor persists the same plan
/// between the runs, and this patch only spares the model the outputs for this
/// one call.
pub(super) fn patch(
    conversation: &Conversation,
    plan: &ClearPlan,
    notes: Option<&str>,
) -> RequestPatch {
    let mut cleared = conversation.clone();
    plan.apply(&mut cleared, notes, rig_memory::summary_author());
    RequestPatch {
        history: Some(cleared.messages().to_vec()),
        ..RequestPatch::default()
    }
}

/// Whether the outputs `plan` drops can be had again by calling the tools again:
/// every dropped output answers a call of a `tracked` tool whose path argument
/// still stats on this machine.
pub(super) fn all_again(
    conversation: &Conversation,
    plan: &ClearPlan,
    tracked: &[TrackArgument],
) -> bool {
    plan.results(conversation).all(|result| {
        call_of(conversation, &result.call).is_some_and(|call| {
            tracked.iter().any(|rule| {
                rule.value(call)
                    .is_some_and(|path| std::path::Path::new(path).exists())
            })
        })
    })
}

/// Whether this clearing asks the model for notes first: near the window, the
/// outputs that cannot be had again are written down before they are dropped.
pub(super) fn notes_needed(
    policy: &CompactionPolicy,
    conversation: &Conversation,
    plan: &ClearPlan,
) -> bool {
    policy.near_window == NearWindow::Summarize && !all_again(conversation, plan, &policy.tracked)
}

/// The request that asks for notes on the outputs `plan` is about to drop: the
/// conversation as it stands, then the notes instruction.
///
/// `None` when the room below the model's input limit is too small to ask: a
/// request that cannot fit asks for nothing, and the clearing is skipped — the
/// summary path keeps the turn.
pub(super) fn notes_request(
    policy: &CompactionPolicy,
    spec: &ModelSpec,
    conversation: &Conversation,
    tokens: u64,
) -> Option<CompletionRequest> {
    let budget = budget(spec, tokens)?;
    Some(
        CompletionRequest::new(Message::user(policy.summarizer.notes_instruction(budget)))
            .preamble(policy.summarizer.prompts.system.clone().into_owned())
            .messages(conversation.messages().iter().cloned())
            .max_tokens(budget),
    )
}

// endregion: --- Functions

// region:    --- Support

/// The call `id` names, in the order the conversation holds it.
fn call_of<'c>(conversation: &'c Conversation, id: &CallId) -> Option<&'c ToolCall> {
    conversation.messages().iter().find_map(|message| {
        let Message::Assistant(turn) = &**message else {
            return None;
        };
        turn.content.iter().find_map(|content| match content {
            AssistantContent::ToolCall(call) if &call.id == id => Some(call),
            _ => None,
        })
    })
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../../test/hook/chain/compaction/notes.rs"]
mod notes_test;

// endregion: --- Tests
