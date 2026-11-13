use super::support::turn_model_spec;
use super::*;

// ---------------------------------------------------------------------------
// Compaction end-to-end test (task 679635b4)
// ---------------------------------------------------------------------------

/// When the conversation exceeds the sliding window, the hook must fire a
/// `ControlCommand::CompactionRequested { source: Auto }` on the control concern. The current turn
/// proceeds with the full history (the orchestrator runs compaction
/// asynchronously; the summary is applied on the next turn via the marker).
#[tokio::test]
async fn compaction_fires_when_conversation_exceeds_window() -> Result<()> {
    let config = test_config();
    let session_id = "test-compaction-fires";

    let bus = crate::bus::Bus::default();
    let mut collector = super::test_utils::BusEventCollector::subscribe(&bus);

    // The compactor needs a model that streams a summary (a plain `text()` model
    // does not serve streaming calls). Script enough streaming turns for the
    // compaction to succeed.
    let compactor_turns: Vec<Vec<rig::test_utils::MockStreamEvent>> = (0..8)
        .map(|_| {
            vec![
                rig::test_utils::MockStreamEvent::Text("summary".to_string()),
                rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let compactor_model = MockCompletionModel::from_stream_turns(compactor_turns);
    let compactor_handle = std::sync::Arc::new(std::sync::Mutex::new(compactor_model.erase()));
    // The compactor plans against the rig-memory store of the session.
    let compactor = crate::conversation::compaction::compactor::NuCompactor::from_shared_model(
        compactor_handle,
        bus.clone(),
        rig_memory::CompactionPolicy::default(),
        crate::conversation::compaction::compactor::resolve_model_spec(
            "openai",
            "gpt-4o",
            Some(8_000),
            Some(1_024),
            None,
        ),
    );

    let mut memory_state = MemoryState::new(crate::session::RigStoreConfig::in_memory());

    // Pre-populate a conversation that far exceeds the token threshold: a large
    // older message the side-call can summarize, then distinct user/assistant
    // pairs. The agent loads its history from the rig store, so the seed goes
    // there. The large message sits after the first one: rig's cut never
    // summarizes the conversation from its very first message.
    let mut seeded: Vec<crate::types::Message> = vec![
        crate::types::Message::user("the opening question"),
        crate::types::Message::assistant("the opening answer"),
        crate::types::Message::user("q".repeat(40_000)),
    ];
    for i in 0..8 {
        seeded.push(crate::types::Message::user(format!("user-{i}")));
        seeded.push(crate::types::Message::assistant(format!("assistant-{i}")));
    }
    seed_rig_store(&memory_state, session_id, seeded).await?;

    // The model must be scripted for every agent-wire call this turn makes: the
    // side-call (it runs on the turn's own model), pass one over the full
    // history, then pass two over the summarized conversation the splice wrote.
    // Clone before moving so we can count the calls after the turn. It answers as
    // a model whose input limit is tiny, so the trigger judges this conversation
    // over it.
    let model = MockCompletionModel::from_stream_turns([
        // The side-call's summary.
        vec![
            MockStreamEvent::Text("summary".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        // Pass 1: the agent's actual response (text).
        vec![
            MockStreamEvent::Text("agent reply".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        // Pass 2: the answer over the summarized conversation.
        vec![
            MockStreamEvent::Text("agent reply".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
    .priced_as(turn_model_spec()?);
    let model_spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let compaction_config = crate::conversation::compaction::CompactionConfig {
        compactor,
        reserve_pct: crate::config::defaults::DEFAULT_RESERVE_PCT,
    };

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            store_latch: Default::default(),
            pending_summary: Default::default(),
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            meter: default_meter(),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        shared_model,
        compaction_config,
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
                task_id: None,
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(
        result.is_ok(),
        "turn must complete; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, ExecuteOutcome::Completed));

    // 1. The ladder completed on the turn: no `ControlCommand::CompactionRequested`
    //    is published (the worker is Manual-only), and the side-call's splice
    //    announced itself as `CompactionStarted` then `CompactionCompleted`.
    let events = collector.drain();
    let saw_requested = events.iter().any(|e| {
        matches!(
            e,
            super::test_utils::TurnTestObservation::Control(
                agnt_kernel::bus::ControlCommand::CompactionRequested { .. }
            )
        )
    });
    assert!(
        !saw_requested,
        "the automatic ladder must not schedule a worker compaction; got: {events:?}"
    );
    let started = events.iter().any(|e| {
        matches!(
            e,
            super::test_utils::TurnTestObservation::Lifecycle(
                agnt_kernel::bus::LifecycleEvent::CompactionStarted {
                    source: agnt_kernel::CompactionSource::Auto
                }
            )
        )
    });
    assert!(
        started,
        "the side-call must announce a started compaction; got: {events:?}"
    );
    let completed = events.iter().any(|e| {
        matches!(
            e,
            super::test_utils::TurnTestObservation::Lifecycle(
                agnt_kernel::bus::LifecycleEvent::CompactionCompleted {
                    source: agnt_kernel::CompactionSource::Auto,
                    ..
                }
            )
        )
    });
    assert!(
        completed,
        "the between-runs splice must announce the compaction it landed; got: {events:?}"
    );

    // 2. The turn made three agent-wire calls: the side-call's summary, then the
    //    answer over the full history, then the answer over the summary.
    assert_eq!(
        model_spy.request_count(),
        3,
        "the side-call and both passes each ask the turn's model once"
    );

    Ok(())
}
