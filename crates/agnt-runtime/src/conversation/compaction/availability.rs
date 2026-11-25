//! Whether the compaction commands have anything to do: the one read of the
//! conversation the sweep's refusal, the undo's refusal, and every surface's
//! command availability answer from.

use agnt_kernel::bus::{CommandFacts, CompactionAvailability};
use rig_memory::{CompactionSettings, summarized_prefix};

use crate::session::CachedMemory;

// region:    --- Constants

/// What a sweep with nothing to summarize answers: the availability reason the
/// manual sweep's refusal prints, and the notice an automatic sweep publishes
/// when its own planner prepares nothing. One text, one condition — the row a
/// palette greys and the line the sweep prints cannot drift.
pub(crate) const NOTHING_TO_SUMMARIZE: &str =
    "Nothing to compact: the conversation holds nothing a sweep would summarize.";

/// What a session with no summary in it answers to the undo: there is nothing to
/// put back. The undo's refusal prints this one text.
pub(crate) const NOTHING_TO_UNDO: &str = "Nothing to undo: the conversation holds no compaction.";

// endregion: --- Constants

// region:    --- Functions

/// Whether the compaction commands of `conversation_id` would do their work.
///
/// The store's conversation is the whole question, and one read answers both
/// commands: a sweep has something to cut exactly while the sweep's own planner
/// prepares a compaction, and the undo has something to put back exactly while a
/// leading summary is in the conversation.
///
/// The compact answer IS the sweep's decision — `prepare_compaction` over the
/// loaded conversation with the session's `keep_recent_tokens` — never a second
/// predicate beside it, so the row a surface dims and the sweep the command runs
/// cannot disagree. The steady state is judged: `overflowed` is `false`, and the
/// sweep passes its own transient overflow verdict when it actually runs.
/// `enabled` does not gate the fact: a manual `/compact` works whether the
/// automatic ladder is on or off.
///
/// This is the one production read of either predicate; the refusals and the
/// published availability fact both derive from the answer returned here.
///
/// A store that cannot be opened is not a settled "nothing to compact": the
/// sweep reports the store refusal itself, so both answers stay `Available`.
pub(crate) async fn command_availability(
    memory: &CachedMemory,
    conversation_id: &str,
    settings: CompactionSettings,
) -> CommandFacts {
    let Ok(store) = memory.rig_store(conversation_id).await else {
        return both_available();
    };
    let log = conversation_id.to_owned();
    let loaded = crate::session::blocking::on_pool(move || rig_memory::load(&*store, &log)).await;
    let Ok(Ok(conversation)) = loaded else {
        return both_available();
    };
    let planned =
        rig_memory::prepare_compaction(&conversation, settings.keep_recent_tokens, false).is_some();
    let compact = if planned {
        CompactionAvailability::Available
    } else {
        CompactionAvailability::Unavailable(NOTHING_TO_SUMMARIZE.to_string())
    };
    let uncompact = if summarized_prefix(&conversation) > 0 {
        CompactionAvailability::Available
    } else {
        CompactionAvailability::Unavailable(NOTHING_TO_UNDO.to_string())
    };
    CommandFacts { compact, uncompact }
}

// endregion: --- Functions

// region:    --- Support

/// The pair a store failure answers: neither command is settled as having nothing
/// to do, so both stay offered and the sweep reports the store refusal itself.
fn both_available() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Available,
        uncompact: CompactionAvailability::Available,
    }
}

// endregion: --- Support
