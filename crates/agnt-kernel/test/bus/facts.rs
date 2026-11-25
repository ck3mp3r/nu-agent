//! Facts-contract tests: latest-wins, droppable snapshots.

use std::collections::BTreeMap;

use crate::bus::{
    Bus, CommandFacts, CompactionAvailability, FactRecord, McpServerUsability, ReceiveError,
    WorkerPhase,
};

use crate::test_support::Result;

/// Folding the surviving suffix of a lagged facts consumer reaches the latest
/// state for every field family. Every record is a whole value, so a lost
/// intermediate snapshot costs no state.
#[tokio::test]
async fn test_facts_lagged_prefix_folds_to_latest_state() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.facts().subscribe();
    const FLOOD: usize = 1_000;

    // -- Exec
    for i in 0..FLOOD {
        let _ = bus
            .facts()
            .publish(FactRecord::ActiveModelIdentity(format!("model-{i}")));
        let _ = bus.facts().publish(FactRecord::McpServerState {
            server: "files".to_string(),
            state: if i % 2 == 0 {
                McpServerUsability::Enabled
            } else {
                McpServerUsability::Failed
            },
            error: None,
        });
        let _ = bus
            .facts()
            .publish(FactRecord::WorkerPhase(if i + 1 == FLOOD {
                WorkerPhase::Idle
            } else {
                WorkerPhase::Busy
            }));
    }
    drop(bus);

    // -- Check
    let mut models: Vec<String> = Vec::new();
    let mut mcp: Option<McpServerUsability> = None;
    let mut phase: Option<WorkerPhase> = None;
    loop {
        match rx.recv().await {
            Ok(fact) => match fact {
                FactRecord::ActiveModelIdentity(model) => models.push(model),
                FactRecord::McpServerState { state, .. } => mcp = Some(state),
                FactRecord::WorkerPhase(next) => phase = Some(next),
                other => panic!("unexpected fact while draining: {other:?}"),
            },
            Err(ReceiveError::Lagged(_)) => continue,
            Err(ReceiveError::Closed) => break,
        }
    }
    assert!(
        !models.is_empty(),
        "the surviving suffix must still carry the model family"
    );
    let newest_model = format!("model-{}", FLOOD - 1);
    assert_eq!(
        models.last().map(String::as_str),
        Some(newest_model.as_str()),
        "the newest snapshot must survive the lag"
    );
    let mut ordered: BTreeMap<u64, ()> = BTreeMap::new();
    for model in &models {
        let index: u64 = model.trim_start_matches("model-").parse()?;
        ordered.insert(index, ());
    }
    let highest = ordered.keys().next_back().ok_or("the last model index")?;
    assert_eq!(
        *highest,
        (FLOOD - 1) as u64,
        "folding the surviving suffix must reach the latest model"
    );
    assert_eq!(
        mcp,
        Some(if (FLOOD - 1).is_multiple_of(2) {
            McpServerUsability::Enabled
        } else {
            McpServerUsability::Failed
        }),
        "folding the surviving suffix must reach the latest MCP state"
    );
    assert_eq!(
        phase,
        Some(WorkerPhase::Idle),
        "folding the surviving suffix must reach the latest worker phase"
    );
    Ok(())
}

/// Without lag, facts arrive in publication order and none is missing — the
/// prefix equivalence the contract promises.
#[tokio::test]
async fn test_facts_under_capacity_arrive_complete() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.facts().subscribe();

    // -- Exec
    for i in 0..8 {
        bus.facts()
            .publish(FactRecord::ActiveModelIdentity(format!("model-{i}")))
            .map_err(|e| format!("facts should accept with a subscriber: {e:?}"))?;
    }
    drop(bus);

    // -- Check
    let mut seen: Vec<String> = Vec::new();
    loop {
        match rx.recv().await {
            Ok(FactRecord::ActiveModelIdentity(model)) => seen.push(model),
            Ok(other) => panic!("unexpected fact: {other:?}"),
            Err(ReceiveError::Lagged(dropped)) => {
                panic!("under capacity, {dropped} dropped is a contract violation")
            }
            Err(ReceiveError::Closed) => break,
        }
    }
    assert_eq!(
        seen,
        (0..8).map(|i| format!("model-{i}")).collect::<Vec<_>>(),
        "under capacity, facts must arrive complete and in order"
    );
    Ok(())
}

/// WHEN a surface holds no answer from the runtime yet, THE default pair SHALL
/// say both gated commands have nothing to do: a session the runtime has not
/// read offers neither, so the pre-fact window cannot show a command that would
/// do nothing.
///
/// The two reasons are the kernel's own pre-fact texts. The runtime's predicate
/// carries the sweep's sentence in its own crate; the kernel cannot name it, and
/// the window that shows this default is the producer of this text.
#[test]
fn test_facts_command_facts_default_is_both_unavailable() -> Result<()> {
    // -- Setup & Fixtures
    let default = CommandFacts::default();

    // -- Check: no command is offered before the runtime answers.
    let compact_reason = match &default.compact {
        CompactionAvailability::Unavailable(reason) => reason,
        CompactionAvailability::Available => {
            return Err("a surface with no answer SHALL NOT offer the sweep".into());
        }
    };
    let uncompact_reason = match &default.uncompact {
        CompactionAvailability::Unavailable(reason) => reason,
        CompactionAvailability::Available => {
            return Err("a surface with no answer SHALL NOT offer the undo".into());
        }
    };

    // -- Check: each reason is the kernel's own sentence for its command.
    assert!(
        compact_reason.starts_with("Nothing to compact"),
        "the sweep's pre-fact reason names the sweep: {compact_reason:?}"
    );
    assert!(
        uncompact_reason.starts_with("Nothing to undo"),
        "the undo's pre-fact reason names the undo: {uncompact_reason:?}"
    );
    assert_ne!(
        compact_reason, uncompact_reason,
        "one reason per command, so a row never prints the other's sentence"
    );
    Ok(())
}
