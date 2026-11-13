//! The in-place summary decision: the request that replaces a turn's history
//! with the kept messages plus the summarizer's instruction, and the plan the
//! reply consumer reads when that call settles.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig::catalog::ModelSpec;
use rig::completion::CompletionResponse;
use rig::conversation::{Conversation, MessageId};
use rig::message::Message;
use rig::run::RequestPatch;
use rig_memory::{
    ClearPlan, CompactReason, CompactionPolicy, Summarizer, SummaryCut, SummaryState,
};

use agnt_kernel::CompactionSource;

use super::PendingClearance;

// region:    --- Types

/// The least output an in-place request is worth asking for: below it the
/// summary would carry too little of the conversation to be useful, so the turn
/// takes the interim scheduling instead.
const LEAST_OUTPUT: u64 = 4_096;

/// How much of the conversation the part being summarized must be, in quarters,
/// for the summary to be worth its own model call.
const WORTH: u64 = 4;

/// What the decision needs beyond the path: the model spec the turn's request is
/// judged against, and the per-turn slot its plans are stashed in.
pub(super) struct InPlace<'a> {
    /// The spec of the model this turn's request goes to; `None` when nothing
    /// is known about its limits.
    pub spec: Option<&'a ModelSpec>,
    /// The per-turn slot the decision stashes its plan in, read when the
    /// patched call settles or between the two runs.
    pub pending: &'a PendingSummary,
}

/// The compaction work this turn stashed: the in-place summary it decided on and
/// the verdict the patched call's reply earned, plus the near-window clearing the
/// executor persists between the two runs.
///
/// One slot for both, because both are the same turn's compaction work and both
/// are read once, by the executor's between-runs splice.
#[derive(Clone, Default)]
pub struct PendingSummary {
    cell: Arc<Mutex<Option<PendingPlan>>>,
    settled: Arc<Mutex<Option<Supersede>>>,
    attempted: Arc<Mutex<bool>>,
    clear: Arc<Mutex<Option<PendingClearance>>>,
}

/// What the decision stashed for the reply consumer.
#[derive(Clone)]
struct PendingPlan {
    /// The cut the patch replaced the request's history with.
    cut: SummaryCut,
    /// Why the summary is being written.
    source: CompactionSource,
    /// The history the patch sent: the kept messages and the instruction.
    request: Vec<Arc<Message>>,
}

/// The patched call's settled reply and the cut it answers: what the turn
/// executor splices between the two runs.
///
/// The reply IS the summary, so the call that settles it validates the answer
/// and leaves the verdict here; the executor then owns the store write. The
/// verdict is data and the text is content: a rejected reply carries its reason
/// beside the text, never instead of it.
#[derive(Clone)]
pub(crate) struct Supersede {
    cut: SummaryCut,
    source: CompactionSource,
    reply: Reply,
}

/// The verdict on the patched call's reply.
#[derive(Clone)]
enum Reply {
    /// The reply is the summary.
    Summary(String),
    /// The reply cannot be used as a summary, and why.
    Rejected { text: String, why: String },
}

// endregion: --- Types

// region:    --- Implementations

impl PendingSummary {
    /// Stash the plan the patched call's reply answers.
    pub fn stash(&self, cut: SummaryCut, source: CompactionSource, request: Vec<Arc<Message>>) {
        let plan = PendingPlan {
            cut,
            source,
            request,
        };
        log::debug!(
            "in-place summary ({:?}): cut at {:?} over {} sent messages",
            plan.source,
            plan.cut.keep_from,
            plan.request.len()
        );
        *self.lock_cell() = Some(plan);
        *self.lock_attempted() = true;
    }

    /// Whether an in-place summary is waiting for its reply.
    pub fn is_pending(&self) -> bool {
        self.lock_cell().is_some()
    }

    /// The source of the summary waiting for its reply, `None` when no call is
    /// patched.
    pub(crate) fn awaiting_reply(&self) -> Option<CompactionSource> {
        self.lock_cell().as_ref().map(|plan| plan.source)
    }

    /// Whether this turn already asked for an in-place summary. One attempt per
    /// turn: a pass that stashed a plan re-runs unpatched, so the summary the
    /// first pass asked for is the turn's only one.
    pub(crate) fn attempted(&self) -> bool {
        *self.lock_attempted()
    }

    /// Take the patched call's reply and the cut it answers, `None` when no
    /// reply settled or it was taken already.
    pub(crate) fn take_settled(&self) -> Option<Supersede> {
        self.lock_settled().take()
    }

    /// Whether the reply that settled cannot be a summary: a side-call then runs
    /// on the same conversation before the turn's second pass.
    pub(crate) fn settled_rejected(&self) -> bool {
        self.lock_settled()
            .as_ref()
            .is_some_and(|settled| settled.rejection().is_some())
    }

    /// Stash the summary a side-call produced: no patched reply carries it, so
    /// the verdict is settled before the call that answers the turn.
    pub(crate) fn stash_side_call(
        &self,
        keep_from: Option<MessageId>,
        state: SummaryState,
        source: CompactionSource,
        summary: String,
    ) {
        log::debug!(
            "side-call summary ({:?}): cut at {keep_from:?} over {} chars",
            source,
            summary.len()
        );
        *self.lock_settled() = Some(Supersede::planned(keep_from, state, source, summary));
        *self.lock_attempted() = true;
    }

    /// Spend this turn's one compaction attempt: a pass that already cleared
    /// re-runs unpatched, so it does not clear again.
    pub(crate) fn spend_attempt(&self) {
        *self.lock_attempted() = true;
    }

    /// Stash the clearing the executor persists between the two runs.
    pub(crate) fn stash_clear(&self, plan: ClearPlan, notes: Option<String>) {
        log::debug!(
            "near-window clearing: {} outputs, {} tokens, notes {}",
            plan.cleared.results,
            plan.cleared.tokens,
            if notes.is_some() { "written" } else { "none" }
        );
        *self.lock_clear() = Some(PendingClearance::new(plan, notes));
    }

    /// Take the clearing the executor persists, `None` when none is stashed.
    pub(crate) fn take_clear(&self) -> Option<PendingClearance> {
        self.lock_clear().take()
    }

    /// Validate the reply of the call that settles and stash its verdict with
    /// the cut it answers. A call whose reply the turn never asked for is left
    /// alone.
    pub(crate) fn settle(&self, response: &CompletionResponse) {
        let Some(plan) = self.lock_cell().take() else {
            return;
        };
        let reply = match Summarizer::summary_text(response) {
            Ok(text) => Reply::Summary(text),
            Err(why) => Reply::Rejected {
                text: response.text(),
                why: why.to_string(),
            },
        };
        log::debug!(
            "in-place summary ({:?}): {} over {} sent messages",
            plan.source,
            match &reply {
                Reply::Summary(text) => format!("validated {} chars", text.len()),
                Reply::Rejected { why, .. } => format!("rejected: {why}"),
            },
            plan.request.len()
        );
        *self.lock_settled() = Some(Supersede {
            cut: plan.cut,
            source: plan.source,
            reply,
        });
    }

    /// Forget everything: the turn ends, or the summary cannot land.
    pub fn clear(&self) {
        *self.lock_cell() = None;
        *self.lock_settled() = None;
        *self.lock_attempted() = false;
        *self.lock_clear() = None;
    }

    /// The cell, recovered if a writer panicked mid-update.
    fn lock_cell(&self) -> MutexGuard<'_, Option<PendingPlan>> {
        self.cell.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The settled-reply cell, recovered if a writer panicked mid-update.
    fn lock_settled(&self) -> MutexGuard<'_, Option<Supersede>> {
        self.settled.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The clearing cell, recovered if a writer panicked mid-update.
    fn lock_clear(&self) -> MutexGuard<'_, Option<PendingClearance>> {
        self.clear.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The spent-attempt cell, recovered if a writer panicked mid-update.
    fn lock_attempted(&self) -> MutexGuard<'_, bool> {
        self.attempted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Supersede {
    /// The summary a side-call produced, with the cut it replaces.
    pub(crate) fn planned(
        keep_from: Option<MessageId>,
        state: SummaryState,
        source: CompactionSource,
        summary: String,
    ) -> Self {
        Self {
            cut: SummaryCut { keep_from, state },
            source,
            reply: Reply::Summary(summary),
        }
    }

    /// Why the summary is being written.
    pub fn source(&self) -> CompactionSource {
        self.source
    }

    /// The summary the patched call produced, `None` when its reply was
    /// rejected.
    pub fn summary(&self) -> Option<&str> {
        match &self.reply {
            Reply::Summary(text) => Some(text),
            Reply::Rejected { .. } => None,
        }
    }

    /// Why the reply cannot be a summary, `None` when it is one.
    pub fn rejection(&self) -> Option<&str> {
        match &self.reply {
            Reply::Summary(_) => None,
            Reply::Rejected { why, .. } => Some(why),
        }
    }

    /// The reply's own text: what identifies its entry in the stored
    /// conversation, empty when the reply carried no text.
    pub fn reply_text(&self) -> &str {
        match &self.reply {
            Reply::Summary(text) => text,
            Reply::Rejected { text, .. } => text,
        }
    }

    /// The first message the summary keeps, by id; `None` when it summarizes
    /// every message.
    pub fn keep_from(&self) -> Option<MessageId> {
        self.cut.keep_from
    }

    /// The summary state the summary goes into: the tracked values the cut
    /// collected, with the summary still empty.
    pub fn state(&self) -> &SummaryState {
        &self.cut.state
    }
}

// endregion: --- Implementations

// region:    --- Functions

/// The in-place request for `conversation`, the cut it makes, and the history
/// it sends: the kept messages plus the summarizer's instruction, with the
/// output budget the request leaves room for.
///
/// `None` when nothing would be summarized, when the part being summarized is
/// too small a share of the conversation to be worth a model call, or when the
/// model leaves less room than the summary needs — the turn then takes the
/// interim scheduling.
pub(super) fn summary(
    conversation: &Conversation,
    spec: &ModelSpec,
    policy: &CompactionPolicy,
) -> Option<(RequestPatch, SummaryCut, Vec<Arc<Message>>)> {
    let state = rig_memory::summary_state(conversation);
    let cut = policy.cut_summary(conversation, &state, &CompactReason::Threshold, spec)?;
    if !worth_summarizing(conversation, policy, spec) {
        return None;
    }
    let keep_from = cut
        .keep_from
        .and_then(|id| conversation.index_of(id))
        .unwrap_or_else(|| conversation.messages().len());
    let kept = conversation.messages().get(keep_from..).unwrap_or_default();
    let budget = budget(spec, rig_memory::estimate_tokens(kept))?;
    let mut request = kept.to_vec();
    request.push(Arc::new(Message::user(
        policy.summarizer.instruction("", budget),
    )));
    let patch = RequestPatch {
        history: Some(request.clone()),
        max_tokens: Some(budget),
        ..RequestPatch::default()
    };
    Some((patch, cut, request))
}

// endregion: --- Functions

// region:    --- Support

/// Whether the part being summarized is enough of the conversation to justify
/// the summary's own model call: rig's rule weighs the cut part against the
/// whole, four to one. A conversation with nothing to cut is worth it.
fn worth_summarizing(
    conversation: &Conversation,
    policy: &CompactionPolicy,
    spec: &ModelSpec,
) -> bool {
    let messages = conversation.messages();
    let from = rig_memory::summarized_prefix(conversation);
    let Some(upto) = policy.cut(messages, from, &CompactReason::Threshold, Some(spec)) else {
        return true;
    };
    let older = rig_memory::estimate_tokens(messages.get(from.min(upto)..upto).unwrap_or_default());
    let whole = rig_memory::estimate_tokens(messages);
    older.saturating_mul(WORTH) >= whole
}

/// The output budget an in-place or notes request asks for: the room its history
/// leaves below the model's input limit. `None` when that room is below the least
/// the answer is worth asking for, which makes the request impossible.
pub(super) fn budget(spec: &ModelSpec, tokens: u64) -> Option<u64> {
    let limit = u64::from(spec.input_limit()?);
    let room = limit.saturating_sub(tokens);
    (room >= LEAST_OUTPUT).then_some(room)
}

// endregion: --- Support
