//! The availability predicate: the one read of the conversation the sweep's
//! refusal, the undo's refusal, and every surface's command availability answer
//! from. One read answers both compaction commands, so the pair is the fact.

use super::super::*;
use super::*;

use crate::conversation::compaction::availability::{NOTHING_TO_SUMMARIZE, NOTHING_TO_UNDO};
use crate::conversation::compaction::{command_availability, undo};
use agnt_kernel::bus::{CommandFacts, CompactionAvailability};

// region:    --- Fixtures

/// The pair a never-compacted conversation answers: a sweep has work, the undo
/// has nothing to put back.
fn never_compacted() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Available,
        uncompact: CompactionAvailability::Unavailable(NOTHING_TO_UNDO.to_string()),
    }
}

/// The pair a conversation ending in its compaction answers: the sweep has
/// nothing left to cut, the summary waits to be undone.
fn compacted() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Unavailable(NOTHING_TO_SUMMARIZE.to_string()),
        uncompact: CompactionAvailability::Available,
    }
}

/// The pair a session holding no conversation answers: a sweep would cut
/// nothing, and there is no summary to put back.
fn both_dim() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Unavailable(NOTHING_TO_SUMMARIZE.to_string()),
        uncompact: CompactionAvailability::Unavailable(NOTHING_TO_UNDO.to_string()),
    }
}

/// The pair a conversation with content after its summary answers: the sweep has
/// the new entry to cut, and the summary still waits to be undone.
fn both_offered() -> CommandFacts {
    CommandFacts {
        compact: CompactionAvailability::Available,
        uncompact: CompactionAvailability::Available,
    }
}

// endregion: --- Fixtures

/// WHEN a conversation has never been compacted, THEN THE pair SHALL offer the
/// sweep and SHALL NOT offer the undo: there is no summary to put back.
#[tokio::test]
async fn test_hook_chain_command_availability_never_compacted_offers_only_the_sweep() -> Result<()>
{
    // -- Setup & Fixtures: a conversation a sweep can summarize.
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;

    // -- Exec & Check: both commands answer from the one read.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        never_compacted(),
        "a live conversation is compactable and holds nothing to undo"
    );
    Ok(())
}

/// WHEN a sweep lands, THEN THE pair SHALL flip in one answer: the sweep is no
/// longer offered and the undo is — the two gates are near-complements over the
/// same state read.
#[tokio::test]
async fn test_hook_chain_command_availability_post_compaction_flips_the_pair() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;

    // -- Exec: the sweep runs and splices its summary in.
    let swept = run_compaction(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;

    // -- Check: the conversation now ends in its most recent compaction.
    assert!(
        swept.is_some(),
        "the fixture's settings summarize the turn, so the sweep ran"
    );
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        compacted(),
        "a session that ends in its compaction offers the undo, not the sweep"
    );
    Ok(())
}

/// WHEN `/uncompact` completes, THEN THE pair SHALL flip back: the summary is
/// gone, so the sweep is offered and the undo is not.
#[tokio::test]
async fn test_hook_chain_command_availability_post_undo_flips_the_pair_back() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;
    let swept = run_compaction(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;
    assert!(swept.is_some(), "the fixture compacts before the undo runs");

    // -- Exec: the undo puts the replaced entries back.
    undo::uncompact(
        "conv-1",
        &fx.memory,
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await
    .map_err(|e| format!("the undo should run: {e}"))?;

    // -- Check: the restored conversation holds no summary, so the pair is the
    // never-compacted one again.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        never_compacted(),
        "an undone session offers the sweep again and holds nothing to undo"
    );
    Ok(())
}

/// WHEN a later turn grows a compacted conversation, THEN THE pair SHALL offer
/// both commands: the sweep has the new entry to cut, and the summary still leads
/// the conversation, waiting to be undone.
#[tokio::test]
async fn test_hook_chain_command_availability_new_messages_after_compaction_offer_both()
-> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;
    let swept = run_compaction(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;
    assert!(
        swept.is_some(),
        "the fixture compacts before the turn lands"
    );

    // -- Exec: a later turn adds to the conversation.
    seed_rig(&fx, &[Message::user("a later turn")]).await?;

    // -- Check: the entry after the summary is something to cut, and the summary
    // itself is still there to undo.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        CommandFacts {
            compact: CompactionAvailability::Available,
            uncompact: CompactionAvailability::Available,
        },
        "an entry after the summary leaves both commands with work to do"
    );
    Ok(())
}

/// WHEN the session holds no conversation at all, THEN THE pair SHALL dim both
/// commands: the sweep's own planner would cut nothing, and no summary waits to
/// be put back. This is the state the surface shows before the first turn, and
/// the user's ruling is that it offers neither command.
#[tokio::test]
async fn test_hook_chain_command_availability_empty_conversation_dims_both() -> Result<()> {
    // -- Setup & Fixtures: a store that opens and holds nothing yet.
    let fx = decide_fixture()?;

    // -- Exec & Check
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        both_dim(),
        "a session with no conversation has nothing to compact and nothing to undo"
    );
    Ok(())
}

/// WHEN a session walks from empty through its first content, its sweep, and the
/// messages that follow it, THEN THE pair SHALL follow the four states the ruling
/// names, one row each. The compact answer is the sweep's own planner decision,
/// so the row a surface dims and the sweep the command runs cannot disagree.
#[tokio::test]
async fn test_hook_chain_command_availability_follows_the_four_states() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;

    // -- Check: state 1 — a session with no conversation offers neither.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        both_dim(),
        "state 1: empty — both dim"
    );

    // -- Setup & Fixtures: state 2 — content a sweep would cut.
    seed_rig(
        &fx,
        &[
            Message::user("turn one"),
            Message::assistant("turn one reply"),
        ],
    )
    .await?;

    // -- Check: state 2 — the sweep has work, the undo has nothing to put back.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        never_compacted(),
        "state 2: content — the sweep only"
    );

    // -- Setup & Fixtures: state 3 — the sweep lands.
    let swept = run_compaction(
        "conv-1",
        &CompactionPath::new(
            fx.memory.as_ref(),
            &fx.compaction,
            &fx.meter,
            &Default::default(),
            &fx.model,
        ),
        CompactionSource::Manual,
        fx.bus.stream(),
        fx.bus.lifecycle(),
    )
    .await;
    assert!(swept.is_some(), "the fixture's settings summarize the turn");

    // -- Check: state 3 — the sweep is spent, the summary waits to be undone.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        compacted(),
        "state 3: just compacted — the undo only"
    );

    // -- Setup & Fixtures: state 4 — a later turn adds content after the summary.
    seed_rig(&fx, &[Message::user("a later turn")]).await?;

    // -- Check: state 4 — both commands have work.
    assert_eq!(
        command_availability(&fx.memory, "conv-1", fx.compaction.settings()).await,
        both_offered(),
        "state 4: summary plus new messages — both offered"
    );
    Ok(())
}
