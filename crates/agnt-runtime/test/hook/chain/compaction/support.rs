//! Fixtures shared by the compaction test modules.
//!
//! Sub-files pull them in with `use super::*;`.

use super::super::*;

use rig::completion::Usage;
use rig::conversation::Conversation;
use rig_memory::Prepared;

/// Seed the fixture's rig-memory store with `messages`.
pub(super) async fn seed_rig(fx: &DecideFixture, messages: &[Message]) -> Result<()> {
    let store = fx
        .memory
        .rig_store("conv-1")
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, "conv-1")?;
    conversation.extend(messages.iter().cloned());
    rig_memory::save(&*store, "conv-1", &mut conversation)?;
    Ok(())
}

/// A request as the decision meters it: a conversation's entries and their ids.
pub(super) fn prepared(conversation: &Conversation) -> Prepared<'_> {
    Prepared {
        model: "gpt-4o",
        prompt: None,
        tools: &[],
        history: conversation,
    }
}

/// A usage reporting both counters and no total: `context_tokens` answers their
/// sum, which is what the trigger reads.
pub(super) fn usage(input: u64, output: u64) -> Usage {
    Usage::new().input_tokens(input).output_tokens(output)
}
