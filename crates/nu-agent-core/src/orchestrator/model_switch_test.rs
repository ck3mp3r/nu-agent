use super::test_shared::*;

#[test]
fn palette_models_does_not_bypass_shared_models_action_path() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
    assert!(runtime.prompts.is_empty());
}

#[test]
fn inline_model_picker_enter_switches_active_model_and_provider() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);
    ui.model_switch_requests
        .push_back("openai/gpt-4o-mini".to_string());

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        ui.active_model_identity,
        Some("openai/gpt-4o-mini".to_string())
    );
}

#[test]
fn model_switch_failure_keeps_previous_model_and_warns() {
    let mut runtime = FakeRuntime {
        switch_model_result: Some(Err("switch failed".to_string())),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);
    ui.model_switch_requests
        .push_back("openai/gpt-4o-mini".to_string());

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        ui.active_model_identity,
        Some("openai/gpt-4o-mini".to_string())
    );
    assert!(ui.warnings.iter().any(|w| w == "switch failed"));
}

#[test]
fn model_switch_uses_cached_startup_plugin_config() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);
    ui.model_switch_requests
        .push_back("openai/gpt-4o-mini".to_string());

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
}

#[test]
fn model_switch_updates_footer_active_model_identity_immediately() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);
    ui.model_switch_requests
        .push_back("openai/gpt-4o-mini".to_string());

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        ui.active_model_identity,
        Some("openai/gpt-4o-mini".to_string())
    );
}

#[test]
fn model_switch_result_artifact_is_rendered() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);
    ui.model_switch_requests
        .push_back("openai/gpt-4o-mini".to_string());

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(
        ui.warnings
            .iter()
            .any(|w| w == "Model switched: openai/gpt-4o-mini")
    );
}

#[test]
fn next_turn_uses_newly_selected_model() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["after-switch"]);
    ui.model_switch_requests
        .push_back("openai/gpt-4o-mini".to_string());

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(runtime.prompts, vec!["after-switch".to_string()]);
}

#[test]
fn model_switch_while_worker_active_is_queued_for_next_turn() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    assert_eq!(
        runtime
            .switched_models
            .lock()
            .expect("switched models lock")
            .as_slice(),
        ["openai/gpt-4o-mini"]
    );
    assert!(
        ui.warnings
            .iter()
            .any(|w| w == "Model switch queued for next turn: openai/gpt-4o-mini")
    );
}

#[test]
fn queued_model_switch_applies_after_current_turn_before_next_dispatch() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
        Arc::clone(&active_pump_count),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime
            .action_log
            .lock()
            .expect("action log lock")
            .as_slice(),
        ["turn:first", "switch:openai/gpt-4o-mini", "turn:second"]
    );
}

#[test]
fn queued_model_switch_last_write_wins() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini", "anthropic/claude-3-5-sonnet"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime
            .switched_models
            .lock()
            .expect("switched models lock")
            .as_slice(),
        ["anthropic/claude-3-5-sonnet"]
    );
}

#[test]
fn queued_model_switch_failure_keeps_previous_model_and_warns() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn))
        .with_switch_model_result(Err("queued switch failed".to_string()));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.active_model_identity(),
        "openai/gpt-4o-mini",
        "failed queued switch must keep previous active identity"
    );
    assert!(ui.warnings.iter().any(|w| w == "queued switch failed"));
}
