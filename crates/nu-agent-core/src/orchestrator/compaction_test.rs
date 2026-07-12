use super::test_shared::*;

#[test]
fn interactive_loop_emits_auto_compaction_when_policy_fires() {
    let mut runtime = FakeRuntime {
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

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::AutoThreshold]
    );
}

#[test]
fn interactive_loop_skips_auto_compaction_when_policy_no_fire() {
    let mut runtime = FakeRuntime {
        auto_decisions: [CompactionTriggerDecision::NoFire {
            reason: "below_lower_bound".to_string(),
        }]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.executed_compaction_sources.is_empty());
}

#[test]
fn interactive_loop_does_not_duplicate_auto_compaction_while_disarmed() {
    let mut runtime = FakeRuntime {
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

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.executed_compaction_sources.len(), 1);
    assert_eq!(
        runtime.executed_compaction_sources[0],
        CompactionTriggerSource::AutoThreshold
    );
}

#[test]
fn auto_compaction_rearms_after_turn_completion() {
    let mut runtime = FakeRuntime {
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
    // 10 pumps: startup eval → dispatch prompt → collect result → re-arm → second eval
    let mut ui = FakeInteractiveUi::with_prompts(&["hello"]).with_min_pump_count(10);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

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

#[test]
fn interactive_loop_continues_turn_processing_with_auto_compaction_enabled() {
    let mut runtime = FakeRuntime {
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

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::AutoThreshold]
    );
}

#[test]
fn recognized_slash_commands_never_sent_to_llm() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/compact",
    ]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::SlashCompact]
    );
}

#[test]
fn models_slash_command_not_sent_to_llm() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
}

#[test]
fn models_slash_command_routes_to_shared_models_action() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
}

#[test]
fn interactive_loop_routes_compact_slash_to_compaction_executor() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact", "hello"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::SlashCompact]
    );
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[test]
fn typed_compact_submit_triggers_compaction_path() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.executed_compaction_sources,
        vec![CompactionTriggerSource::SlashCompact]
    );
    assert!(runtime.prompts.is_empty());
}

#[test]
fn interactive_loop_unknown_slash_emits_warning_and_continues() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact now", "real prompt"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(
        ui.warnings
            .iter()
            .any(|entry| entry == "Unknown slash command: /compact now")
    );
    assert_eq!(runtime.prompts, vec!["real prompt".to_string()]);
}

#[test]
fn recognized_slash_commands_not_persisted_as_session_turn_messages() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/compact"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
    assert!(
        runtime.executed_compaction_sources == vec![CompactionTriggerSource::SlashCompact],
        "only /compact should route to compaction trigger"
    );
}

#[test]
fn manual_and_auto_compaction_failure_surface_is_consistent() {
    let mut runtime = FakeRuntime {
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

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

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

#[test]
fn slash_commands_reuse_command_palette_action_handlers() {
    let mut runtime = FakeRuntime::default();
    let mut ui =
        FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/models", "/agent"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

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

#[test]
fn command_palette_models_action_opens_inline_model_picker() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
}

#[test]
fn manual_and_auto_compaction_share_single_execution_path() {
    let mut runtime = FakeRuntime {
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

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

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
