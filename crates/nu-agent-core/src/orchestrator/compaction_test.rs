use super::test_shared::*;

#[tokio::test]
async fn interactive_loop_emits_auto_compaction_when_policy_fires() {
    let runtime = FakeRuntime {
        auto_decisions: [CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::AutoThreshold]
    );
}

#[tokio::test]
async fn interactive_loop_skips_auto_compaction_when_policy_no_fire() {
    let runtime = FakeRuntime {
        auto_decisions: [CompactionTriggerDecision::NoFire {
            reason: "below_lower_bound".to_string(),
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.executed_compaction_sources.is_empty());
}

#[tokio::test]
async fn interactive_loop_does_not_duplicate_auto_compaction_while_disarmed() {
    let runtime = FakeRuntime {
        auto_decisions: [CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.executed_compaction_sources.len(), 1);
    assert_eq!(
        runtime.executed_compaction_sources[0],
        CompactionTriggerSource::AutoThreshold
    );
}

#[tokio::test]
async fn auto_compaction_rearms_after_turn_completion() {
    let runtime = FakeRuntime {
        auto_decisions: [
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "threshold_reached".to_string(),
                strategy: CompactionStrategy::SlidingSummary,
            },
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "threshold_reached_again".to_string(),
                strategy: CompactionStrategy::SlidingSummary,
            },
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&["hello"]).with_min_pump_count(10);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["hello".to_string()],
        "prompt must be processed"
    );
    assert_eq!(
        runtime.auto_decisions.len(),
        0,
        "all decisions must be consumed"
    );
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![
            CompactionTriggerSource::AutoThreshold,
            CompactionTriggerSource::AutoThreshold,
        ]
    );
}

#[tokio::test]
async fn interactive_loop_continues_turn_processing_with_auto_compaction_enabled() {
    let runtime = FakeRuntime {
        auto_decisions: [CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&["hello"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::AutoThreshold]
    );
}

#[tokio::test]
async fn recognized_slash_commands_never_sent_to_llm() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/compact",
    ]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::SlashCompact]
    );
}

#[tokio::test]
async fn models_slash_command_not_sent_to_llm() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
}

#[tokio::test]
async fn models_slash_command_routes_to_shared_models_action() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
}

#[tokio::test]
async fn interactive_loop_routes_compact_slash_to_compaction_executor() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact", "hello"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::SlashCompact]
    );
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[tokio::test]
async fn typed_compact_submit_triggers_compaction_path() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::SlashCompact]
    );
    assert!(runtime.prompts.is_empty());
}

#[tokio::test]
async fn interactive_loop_unknown_slash_emits_warning_and_continues() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact now", "real prompt"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(
        ui.warnings
            .iter()
            .any(|entry| entry == "Unknown slash command: /compact now")
    );
    assert_eq!(runtime.prompts, vec!["real prompt".to_string()]);
}

#[tokio::test]
async fn recognized_slash_commands_not_persisted_as_session_turn_messages() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/compact"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
    assert!(
        runtime.executed_compaction_sources == vec![CompactionTriggerSource::SlashCompact],
        "only /compact should route to compaction trigger"
    );
}

#[tokio::test]
async fn manual_and_auto_compaction_failure_surface_is_consistent() {
    let runtime = FakeRuntime {
        fail_compaction: true,
        auto_decisions: [CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(
        runtime.compaction_call_count >= 1,
        "expected compaction executor to be invoked at least once"
    );
    assert!(ui.warnings.iter().all(|w| {
        !w.starts_with("Session compaction failed:")
            || w.as_str() == "Session compaction failed: sliding_summary summarization unavailable"
    }));
}

#[tokio::test]
async fn slash_commands_reuse_command_palette_action_handlers() {
    let runtime = FakeRuntime::default();
    let mut ui =
        FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/models", "/agent"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions,
        vec![
            SharedUiAction::Help,
            SharedUiAction::Status,
            SharedUiAction::Mcps,
            SharedUiAction::Models,
            SharedUiAction::Agents,
        ]
    );
    assert!(runtime.prompts.is_empty());
}

#[tokio::test]
async fn command_palette_models_action_opens_inline_model_picker() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
}

#[tokio::test]
async fn manual_and_auto_compaction_share_single_execution_path() {
    let runtime = FakeRuntime {
        auto_decisions: [CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.compaction_call_count, 2);
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![
            CompactionTriggerSource::AutoThreshold,
            CompactionTriggerSource::SlashCompact
        ]
    );
}
