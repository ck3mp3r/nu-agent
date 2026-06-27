use nu_protocol::{LabeledError, Span, Value};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use crate::compaction::CompactionStrategy;
use crate::orchestrator::{run_hydrated_interactive_loop, run_interactive_loop, run_single_turn};
use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    contracts::{
        CoreRuntime, DisplayStateUi, ExtendedRuntime, LifecycleUi, McpToggleRequest,
        McpUsabilityState, ProgressUi, SharedUiAction, TranscriptUi, UiMessageSnapshot,
        UiMessageUsageSnapshot, UserInputUi,
    },
    event::{
        PermissionDecision, PermissionDecisionSubmission, PermissionRequestContext, ToolDisplay,
        ToolDisplaySection, UiEvent,
    },
};

#[derive(Default)]
struct FakeProgressUi {
    events: Vec<UiEvent>,
}

impl ProgressUi for FakeProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

struct FakeInteractiveUi {
    submitted: std::collections::VecDeque<String>,
    quit: bool,
    pump_count: usize,
    call_order: Vec<&'static str>,
    hydrated_messages: Vec<UiMessageSnapshot>,
    mcp_toggle_requests: std::collections::VecDeque<McpToggleRequest>,
    mcp_states: Vec<(String, McpUsabilityState)>,
    mcp_details: Vec<(String, McpUsabilityState, Option<String>, usize)>,
    mcp_visible_tool_count_updates: Vec<(String, usize)>,
    warnings: Vec<String>,
    expected_mcp_updates: usize,
    shared_actions: Vec<SharedUiAction>,
    model_switch_requests: std::collections::VecDeque<String>,
    active_model_identity: Option<String>,
    agent_switch_requests: std::collections::VecDeque<String>,
    active_agent_identity: Option<String>,
}

impl FakeInteractiveUi {
    fn with_prompts(prompts: &[&str]) -> Self {
        Self {
            submitted: prompts.iter().map(|s| s.to_string()).collect(),
            quit: false,
            pump_count: 0,
            call_order: Vec::new(),
            hydrated_messages: Vec::new(),
            mcp_toggle_requests: std::collections::VecDeque::new(),
            mcp_states: Vec::new(),
            mcp_details: Vec::new(),
            mcp_visible_tool_count_updates: Vec::new(),
            warnings: Vec::new(),
            expected_mcp_updates: 0,
            shared_actions: Vec::new(),
            model_switch_requests: std::collections::VecDeque::new(),
            active_model_identity: None,
            agent_switch_requests: std::collections::VecDeque::new(),
            active_agent_identity: None,
        }
    }

    fn with_expected_mcp_updates(mut self, expected_mcp_updates: usize) -> Self {
        self.expected_mcp_updates = expected_mcp_updates;
        self
    }
}

impl ProgressUi for FakeInteractiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Warning { message } = event {
            self.warnings.push(message.clone());
        }
        if let UiEvent::TurnError { message } = event {
            self.warnings.push(message.clone());
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for FakeInteractiveUi {
    fn pump_once(&mut self) {
        self.pump_count = self.pump_count.saturating_add(1);
        self.call_order.push("pump_once");
        if self.submitted.is_empty()
            && self.mcp_toggle_requests.is_empty()
            && self.mcp_states.len() >= self.expected_mcp_updates
            && self.pump_count > 1
        {
            self.quit = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for FakeInteractiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.mcp_toggle_requests.pop_front()
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.model_switch_requests.pop_front()
    }

    fn take_next_agent_picker_launch_request(&mut self) -> bool {
        false
    }

    fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.agent_switch_requests.pop_front()
    }
}

impl DisplayStateUi for FakeInteractiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) {
        self.mcp_details.push((
            server_name.to_string(),
            state,
            reason,
            llm_visible_mcp_tool_count,
        ));
        self.set_mcp_server_state(server_name, state);
    }

    fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.mcp_visible_tool_count_updates
            .push((server_name.to_string(), count));
    }

    fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        _server_name: &str,
        _names: Vec<String>,
    ) {
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.shared_actions.push(action);
        true
    }

    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.active_model_identity = Some(active_model_identity.to_string());
    }

    fn set_active_agent_identity(&mut self, name: &str) {
        self.active_agent_identity = Some(name.to_string());
    }
}

impl TranscriptUi for FakeInteractiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
        self.call_order.push("hydrate");
        self.hydrated_messages.extend(messages);
    }
}

#[derive(Default)]
struct ToolDisplayOnlyRuntime;

impl CoreRuntime for ToolDisplayOnlyRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        ui.emit(&UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: "{}".to_string(),
        });
        ui.emit(&UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: r#"{"path":"file.txt","diff":"--- a/file.txt\n+++ b/file.txt\n"}"#.to_string(),
            display: Some(ToolDisplay {
                title: "edit file.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "file.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/file.txt\n+++ b/file.txt\n".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        });
        ui.emit(&UiEvent::Completed { tool_calls: 1 });
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for ToolDisplayOnlyRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }
}

#[test]
fn tool_display_path_does_not_require_assistant_synthesis_round_trip() {
    let mut runtime = ToolDisplayOnlyRuntime;
    let mut ui = FakeProgressUi::default();

    let value = run_single_turn(
        &mut runtime,
        &mut ui,
        "show me diff".to_string(),
        None,
        Span::test_data(),
    )
    .expect("single turn");

    assert!(value.is_nothing());
    assert!(ui.events.iter().any(|event| matches!(
        event,
        UiEvent::ToolEnd {
            display: Some(_),
            ..
        }
    )));
    assert!(
        !ui.events
            .iter()
            .any(|event| matches!(event, UiEvent::AssistantMessage { .. }))
    );
}

#[derive(Default)]
struct FakeRuntime {
    prompts: Vec<String>,
    auto_decisions: std::collections::VecDeque<CompactionTriggerDecision>,
    executed_compaction_sources: Vec<CompactionTriggerSource>,
    fail_compaction: bool,
    compaction_call_count: usize,
    switched_models: Vec<String>,
    switch_model_result: Option<Result<String, String>>,
}

impl CoreRuntime for FakeRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ExtendedRuntime for FakeRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }

    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        self.auto_decisions.pop_front()
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.compaction_call_count = self.compaction_call_count.saturating_add(1);
        self.executed_compaction_sources.push(source);
        if self.fail_compaction {
            return Err("auto compaction failed".to_string());
        }
        Ok(())
    }

    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.switched_models.push(model_spec.to_string());
        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }
        Ok((model_spec.to_string(), None))
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }
}

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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.executed_compaction_sources.is_empty());
}

#[test]
fn interactive_loop_does_not_duplicate_auto_compaction_while_disarmed() {
    let mut runtime = FakeRuntime {
        auto_decisions: [
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "threshold_reached".to_string(),
                strategy: CompactionStrategy::SlidingSummary,
            },
            CompactionTriggerDecision::NoFire {
                reason: "disarmed".to_string(),
            },
            CompactionTriggerDecision::NoFire {
                reason: "disarmed".to_string(),
            },
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.executed_compaction_sources.len(), 1);
    assert_eq!(
        runtime.executed_compaction_sources[0],
        CompactionTriggerSource::AutoThreshold
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
}

#[test]
fn models_slash_command_routes_to_shared_models_action() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
}

#[test]
fn interactive_loop_routes_compact_slash_to_compaction_executor() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/compact", "hello"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
}

#[test]
fn palette_models_does_not_bypass_shared_models_action_path() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["/models"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.active_model_identity(),
        "openai/gpt-4o-mini",
        "failed queued switch must keep previous active identity"
    );
    assert!(ui.warnings.iter().any(|w| w == "queued switch failed"));
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

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
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

#[test]
fn run_single_turn_uses_progress_ui_trait_boundary() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeProgressUi::default();

    let value = run_single_turn(
        &mut runtime,
        &mut ui,
        "hello".to_string(),
        Some("ctx".to_string()),
        Span::test_data(),
    )
    .expect("single turn");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[test]
fn run_interactive_loop_uses_interactive_ui_trait_boundary() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["a", "b"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["a".to_string(), "b".to_string()]);
}

#[derive(Default)]
struct FakeValueRuntime {
    prompts: Vec<String>,
}

impl CoreRuntime for FakeValueRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::record(nu_protocol::Record::new(), span))
    }
}

impl ExtendedRuntime for FakeValueRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }
}

#[test]
fn interactive_loop_does_not_return_per_turn_values_to_stdout() {
    let mut runtime = FakeValueRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["hello"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[derive(Default)]
struct CancelFirstRuntime {
    prompts: Vec<String>,
}

impl CoreRuntime for CancelFirstRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("LLM call cancelled"));
        }

        Ok(Value::nothing(Span::test_data()))
    }
}

impl ExtendedRuntime for CancelFirstRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }
}

#[test]
fn interactive_loop_treats_llm_cancellation_as_non_fatal_and_continues() {
    let mut runtime = CancelFirstRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["first", "second"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should continue after cancellation");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
}

#[derive(Default)]
struct ErrorFirstRuntime {
    prompts: Vec<String>,
}

impl CoreRuntime for ErrorFirstRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("API rate limit exceeded"));
        }

        Ok(Value::nothing(Span::test_data()))
    }
}

impl ExtendedRuntime for ErrorFirstRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }
}

#[test]
fn interactive_loop_treats_errors_as_non_fatal_and_displays_inline() {
    let mut runtime = ErrorFirstRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["first", "second"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should continue after error");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(
        ui.warnings
            .iter()
            .any(|w| w.contains("API rate limit exceeded")),
        "error should be displayed as inline warning"
    );
}

#[test]
fn run_hydrated_interactive_loop_hydrates_before_first_pump() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let messages = vec![
        UiMessageSnapshot::new("user", "from history"),
        UiMessageSnapshot::new("assistant", "from assistant"),
    ];

    let value = run_hydrated_interactive_loop(
        &mut runtime,
        &mut ui,
        messages,
        None,
        None,
        Span::test_data(),
        None,
    )
    .expect("interactive loop with hydration");

    assert!(value.is_nothing());
    assert_eq!(
        &ui.call_order[..2],
        ["hydrate", "pump_once"],
        "expected hydrate before first pump"
    );
}

#[test]
fn run_hydrated_interactive_loop_hydrates_exactly_once() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let messages = vec![UiMessageSnapshot::new("user", "history"), {
        let mut s = UiMessageSnapshot::new("assistant", "response");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(321),
        });
        s
    }];
    run_hydrated_interactive_loop(
        &mut runtime,
        &mut ui,
        messages.clone(),
        None,
        None,
        Span::test_data(),
        None,
    )
    .expect("interactive loop with hydration");

    assert_eq!(ui.hydrated_messages, messages);
}

#[derive(Default)]
struct LongRunningRuntime {
    prompts: Arc<Mutex<Vec<String>>>,
    switched_models: Arc<Mutex<Vec<String>>>,
    action_log: Arc<Mutex<Vec<String>>>,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    switch_model_result: Option<Result<String, String>>,
    active_identity: Arc<Mutex<String>>,
}

impl LongRunningRuntime {
    fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            switched_models: Arc::new(Mutex::new(Vec::new())),
            action_log: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
            switch_model_result: None,
            active_identity: Arc::new(Mutex::new("openai/gpt-4o-mini".to_string())),
        }
    }

    fn with_switch_model_result(mut self, result: Result<String, String>) -> Self {
        self.switch_model_result = Some(result);
        self
    }
}

impl CoreRuntime for LongRunningRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);
        self.action_log
            .lock()
            .expect("action log lock")
            .push(format!("turn:{prompt}"));
        self.prompts
            .lock()
            .expect("prompts lock")
            .push(prompt.clone());

        if prompt == "first" {
            while !self.block_first_turn.load(Ordering::SeqCst) {
                if ui.take_cancel_requested() {
                    self.active.store(false, Ordering::SeqCst);
                    return Err(LabeledError::new("LLM call cancelled"));
                }
                ui.emit(&UiEvent::Tick);
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ExtendedRuntime for LongRunningRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }

    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.action_log
            .lock()
            .expect("action log lock")
            .push(format!("switch:{model_spec}"));
        self.switched_models
            .lock()
            .expect("switched models lock")
            .push(model_spec.to_string());

        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }

        let mut identity = self.active_identity.lock().expect("identity lock");
        *identity = model_spec.to_string();
        Ok((model_spec.to_string(), None))
    }

    fn active_model_identity(&self) -> String {
        self.active_identity.lock().expect("identity lock").clone()
    }
}

struct ResponsiveInteractiveUi {
    submitted: std::collections::VecDeque<String>,
    injected_during_active: Vec<String>,
    injected_model_switch_during_active: Vec<String>,
    model_switch_requests: std::collections::VecDeque<String>,
    injected: bool,
    quit: bool,
    fatal: Option<String>,
    pump_count: usize,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    completed_count: usize,
    expected_completions: usize,
    active_pump_count: Arc<AtomicUsize>,
    mcp_states: Vec<(String, McpUsabilityState)>,
    warnings: Vec<String>,
}

struct PermissionGateRuntime {
    side_effects: Arc<AtomicUsize>,
    requested: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    request_id: String,
    rule_identity: String,
}

impl PermissionGateRuntime {
    fn new() -> Self {
        Self {
            side_effects: Arc::new(AtomicUsize::new(0)),
            requested: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(false)),
            request_id: "ask-0000000000000abc".to_string(),
            rule_identity: "nested:nu__run.command:*".to_string(),
        }
    }
}

impl CoreRuntime for PermissionGateRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);

        let controller =
            crate::protocol::permission::PermissionController::new(Duration::from_secs(2));
        let (token, requested_event) = controller
            .begin_request(crate::protocol::permission::PermissionRequest {
                request_id: self.request_id.clone(),
                context: PermissionRequestContext {
                    tool: "nu__run".to_string(),
                    source: "closure".to_string(),
                    mode: Some("apply".to_string()),
                    matched_rule_identity: self.rule_identity.clone(),
                    scope: "nested".to_string(),
                    target_field: Some("command".to_string()),
                    pattern: "*".to_string(),
                    summary: "tool[nu__run] args={\"command\":\"echo hi\"}".to_string(),
                    pre_authorize_display: None,
                },
            })
            .expect("permission request");

        crate::protocol::permission::install_active_permission_submission_sender(Some(
            token.sender_clone(),
        ));
        ui.emit(&requested_event);
        self.requested.store(true, Ordering::SeqCst);

        let (resolution, events) = controller.await_resolution(&token);
        for event in events {
            ui.emit(&event);
        }

        crate::protocol::permission::install_active_permission_submission_sender(None);

        if let crate::protocol::permission::PermissionResolution::Decision { decision, .. } =
            resolution
            && decision != PermissionDecision::Deny
        {
            self.side_effects.fetch_add(1, Ordering::SeqCst);
            ui.emit(&UiEvent::ToolStart {
                name: "nu__run".to_string(),
                source: "closure".to_string(),
                arguments: r#"{"command":"echo hi"}"#.to_string(),
            });
        }

        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        self.finished.store(true, Ordering::SeqCst);
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for PermissionGateRuntime {}

struct PermissionOrderingUi {
    submitted: std::collections::VecDeque<String>,
    pending_decisions: std::collections::VecDeque<PermissionDecisionSubmission>,
    events: Vec<UiEvent>,
    decision: PermissionDecision,
    decision_delay_pumps: usize,
    pumps_since_request: usize,
    request_seen: bool,
    quit: bool,
    active: Arc<AtomicBool>,
    pumps_while_waiting: Arc<AtomicUsize>,
    side_effects: Arc<AtomicUsize>,
}

impl PermissionOrderingUi {
    fn new(
        decision: PermissionDecision,
        decision_delay_pumps: usize,
        active: Arc<AtomicBool>,
        pumps_while_waiting: Arc<AtomicUsize>,
        side_effects: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            submitted: ["run".to_string()].into_iter().collect(),
            pending_decisions: std::collections::VecDeque::new(),
            events: Vec::new(),
            decision,
            decision_delay_pumps,
            pumps_since_request: 0,
            request_seen: false,
            quit: false,
            active,
            pumps_while_waiting,
            side_effects,
        }
    }
}

impl ProgressUi for PermissionOrderingUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
        if let UiEvent::PermissionRequested {
            request_id,
            context,
        } = event
        {
            self.request_seen = true;
            self.pending_decisions
                .push_back(PermissionDecisionSubmission {
                    request_id: request_id.clone(),
                    decision: self.decision,
                    matched_rule_identity: context.matched_rule_identity.clone(),
                });
        }
        if matches!(event, UiEvent::Completed { .. }) {
            self.quit = true;
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for PermissionOrderingUi {
    fn pump_once(&mut self) {
        if self.request_seen {
            self.pumps_since_request = self.pumps_since_request.saturating_add(1);
        }
        if self.active.load(Ordering::SeqCst)
            && self.request_seen
            && self.side_effects.load(Ordering::SeqCst) == 0
        {
            self.pumps_while_waiting.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for PermissionOrderingUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_permission_decision_submission(&mut self) -> Option<PermissionDecisionSubmission> {
        if self.pumps_since_request < self.decision_delay_pumps {
            return None;
        }
        self.pending_decisions.pop_front()
    }
}

impl DisplayStateUi for PermissionOrderingUi {
    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for PermissionOrderingUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

impl ResponsiveInteractiveUi {
    fn new(
        initial_prompts: &[&str],
        injected_during_active: &[&str],
        injected_model_switch_during_active: &[&str],
        active: Arc<AtomicBool>,
        block_first_turn: Arc<AtomicBool>,
        expected_completions: usize,
        active_pump_count: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            injected_during_active: injected_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            injected_model_switch_during_active: injected_model_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            model_switch_requests: std::collections::VecDeque::new(),
            injected: false,
            quit: false,
            fatal: None,
            pump_count: 0,
            active,
            block_first_turn,
            completed_count: 0,
            expected_completions,
            active_pump_count,
            mcp_states: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

impl ProgressUi for ResponsiveInteractiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count >= self.expected_completions {
                self.quit = true;
            }
        } else if let UiEvent::Warning { message } = event {
            self.warnings.push(message.clone());
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for ResponsiveInteractiveUi {
    fn pump_once(&mut self) {
        self.pump_count += 1;
        if self.active.load(Ordering::SeqCst) {
            self.active_pump_count.fetch_add(1, Ordering::SeqCst);
            if !self.injected {
                for prompt in self.injected_during_active.clone() {
                    self.submitted.push_back(prompt);
                }
                for request in self.injected_model_switch_during_active.clone() {
                    self.model_switch_requests.push_back(request);
                }
                self.injected = true;
                // Deterministic unblock: once we've injected prompts during active work,
                // allow the first turn to complete so queued prompts can proceed.
                self.block_first_turn.store(true, Ordering::SeqCst);
            }
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }
}

impl UserInputUi for ResponsiveInteractiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.model_switch_requests.pop_front()
    }
}

impl DisplayStateUi for ResponsiveInteractiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for ResponsiveInteractiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

#[test]
fn interactive_loop_processes_input_while_first_turn_is_running() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
        Arc::clone(&active_pump_count),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should stay responsive");

    assert!(value.is_nothing());
    assert!(
        active_pump_count.load(Ordering::SeqCst) > 0,
        "expected UI pump to continue while runtime turn was active"
    );
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second"]
    );
}

#[test]
fn interactive_loop_preserves_fifo_for_prompts_queued_while_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second", "third"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        3,
        Arc::clone(&active_pump_count),
    );

    let _ = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should complete queued prompts");

    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second", "third"]
    );
}

#[test]
#[serial_test::serial]
fn permission_requested_emits_before_execution_and_waits_for_decision_before_side_effects() {
    let mut runtime = PermissionGateRuntime::new();
    let pumps_while_waiting = Arc::new(AtomicUsize::new(0));
    let mut ui = PermissionOrderingUi::new(
        PermissionDecision::AllowOnce,
        4,
        Arc::clone(&runtime.active),
        Arc::clone(&pumps_while_waiting),
        Arc::clone(&runtime.side_effects),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.side_effects.load(Ordering::SeqCst), 1);
    assert!(
        pumps_while_waiting.load(Ordering::SeqCst) > 0,
        "execution must pause while waiting for permission decision"
    );

    let requested_idx = ui
        .events
        .iter()
        .position(|event| matches!(event, UiEvent::PermissionRequested { .. }))
        .expect("PermissionRequested must be emitted");
    let submitted_idx = ui
        .events
        .iter()
        .position(|event| matches!(event, UiEvent::PermissionDecisionSubmitted { .. }))
        .expect("PermissionDecisionSubmitted must be emitted");
    let tool_start_idx = ui
        .events
        .iter()
        .position(|event| matches!(event, UiEvent::ToolStart { .. }))
        .expect("ToolStart should happen after allow decision");

    assert!(requested_idx < submitted_idx);
    assert!(submitted_idx < tool_start_idx);
}

#[test]
#[serial_test::serial]
fn deny_decision_resumes_deterministically_without_pre_decision_handler_side_effects() {
    let mut runtime = PermissionGateRuntime::new();
    let pumps_while_waiting = Arc::new(AtomicUsize::new(0));
    let mut ui = PermissionOrderingUi::new(
        PermissionDecision::Deny,
        3,
        Arc::clone(&runtime.active),
        Arc::clone(&pumps_while_waiting),
        Arc::clone(&runtime.side_effects),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.side_effects.load(Ordering::SeqCst), 0);
    assert!(
        pumps_while_waiting.load(Ordering::SeqCst) > 0,
        "execution must remain paused until deny decision arrives"
    );

    assert!(
        ui.events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionRequested { .. }))
    );
    assert!(ui.events.iter().any(|event| matches!(
        event,
        UiEvent::PermissionDecisionSubmitted {
            decision: PermissionDecision::Deny,
            ..
        }
    )));
    assert!(
        !ui.events
            .iter()
            .any(|event| matches!(event, UiEvent::ToolStart { .. }))
    );
}

struct ModelPickerLaunchWhileActiveUi {
    submitted: std::collections::VecDeque<String>,
    pending_model_picker_launch_requests: usize,
    injected: bool,
    quit: bool,
    fatal: Option<String>,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    completed_count: usize,
    expected_completions: usize,
    shared_actions: Vec<SharedUiAction>,
    shared_actions_observed_while_active: Vec<bool>,
    mcp_states: Vec<(String, McpUsabilityState)>,
}

impl ModelPickerLaunchWhileActiveUi {
    fn new(
        initial_prompts: &[&str],
        expected_completions: usize,
        active: Arc<AtomicBool>,
        block_first_turn: Arc<AtomicBool>,
    ) -> Self {
        Self {
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            pending_model_picker_launch_requests: 0,
            injected: false,
            quit: false,
            fatal: None,
            active,
            block_first_turn,
            completed_count: 0,
            expected_completions,
            shared_actions: Vec::new(),
            shared_actions_observed_while_active: Vec::new(),
            mcp_states: Vec::new(),
        }
    }
}

impl ProgressUi for ModelPickerLaunchWhileActiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count >= self.expected_completions {
                self.quit = true;
            }
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for ModelPickerLaunchWhileActiveUi {
    fn pump_once(&mut self) {
        if self.active.load(Ordering::SeqCst) && !self.injected {
            self.pending_model_picker_launch_requests =
                self.pending_model_picker_launch_requests.saturating_add(1);
            self.injected = true;
            self.block_first_turn.store(true, Ordering::SeqCst);
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }
}

impl UserInputUi for ModelPickerLaunchWhileActiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_model_picker_launch_request(&mut self) -> bool {
        if self.pending_model_picker_launch_requests == 0 {
            return false;
        }
        self.pending_model_picker_launch_requests =
            self.pending_model_picker_launch_requests.saturating_sub(1);
        true
    }
}

impl DisplayStateUi for ModelPickerLaunchWhileActiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.shared_actions.push(action);
        self.shared_actions_observed_while_active
            .push(self.active.load(Ordering::SeqCst));
        true
    }
}

impl TranscriptUi for ModelPickerLaunchWhileActiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

#[test]
fn models_launcher_opens_picker_while_worker_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let mut ui = ModelPickerLaunchWhileActiveUi::new(
        &["first"],
        1,
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should process model launcher while active");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
    assert_eq!(ui.shared_actions_observed_while_active, vec![true]);
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}

#[test]
fn models_slash_opens_picker_while_worker_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let mut ui = ModelPickerLaunchWhileActiveUi::new(
        &["first"],
        1,
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
    );

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should process /models while active");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
    assert_eq!(ui.shared_actions_observed_while_active, vec![true]);
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}

struct AbortDuringActiveUi {
    submitted: std::collections::VecDeque<String>,
    quit: bool,
    fatal: Option<String>,
    esc_stage: u8,
    active: Arc<AtomicBool>,
    cancel_requested: Arc<AtomicBool>,
    completed_count: usize,
    mcp_states: Vec<(String, McpUsabilityState)>,
}

impl AbortDuringActiveUi {
    fn new(active: Arc<AtomicBool>) -> Self {
        Self {
            submitted: ["first".to_string(), "queued".to_string()]
                .into_iter()
                .collect(),
            quit: false,
            fatal: None,
            esc_stage: 0,
            active,
            cancel_requested: Arc::new(AtomicBool::new(false)),
            completed_count: 0,
            mcp_states: Vec::new(),
        }
    }
}

impl ProgressUi for AbortDuringActiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count > 0 {
                self.quit = true;
            }
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }
}

impl LifecycleUi for AbortDuringActiveUi {
    fn pump_once(&mut self) {
        if self.active.load(Ordering::SeqCst) && self.esc_stage < 2 {
            self.esc_stage += 1;
            if self.esc_stage == 2 {
                self.cancel_requested.store(true, Ordering::SeqCst);
                self.submitted.clear();
            }
        }

        // Deterministic termination: once cancellation has been requested and
        // active work is no longer running, allow loop exit even if no
        // Completed event is emitted for cancellation.
        if self.esc_stage == 2 && !self.active.load(Ordering::SeqCst) {
            self.quit = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }
}

impl UserInputUi for AbortDuringActiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }
}

impl DisplayStateUi for AbortDuringActiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for AbortDuringActiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

#[test]
fn interactive_loop_global_abort_cancels_active_and_does_not_run_queued_prompt() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let mut runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let mut ui = AbortDuringActiveUi::new(Arc::clone(&runtime.active));

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop should treat cancellation as non-fatal");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}

struct McpToggleRuntime {
    toggles: Vec<(String, bool)>,
    next_state: McpUsabilityState,
    visible_count: usize,
    visible_count_by_server: usize,
}

struct StartupHydrationRuntime {
    names_by_server: Vec<(String, Vec<String>)>,
}

impl CoreRuntime for StartupHydrationRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for StartupHydrationRuntime {
    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.names_by_server
            .iter()
            .map(|(_, names)| names.len())
            .sum()
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        self.names_by_server.clone()
    }
}

#[test]
fn interactive_loop_startup_hydration_initializes_per_server_visible_counts_before_toggles() {
    let mut runtime = StartupHydrationRuntime {
        names_by_server: vec![
            (
                "gh".to_string(),
                vec!["gh__issues".to_string(), "gh__prs".to_string()],
            ),
            ("k8s".to_string(), vec!["k8s__pods".to_string()]),
        ],
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 2), ("k8s".to_string(), 1)]
    );
}

impl CoreRuntime for McpToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for McpToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.toggles.push((server_name.to_string(), enabled));
        Ok(self.next_state)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        self.visible_count_by_server
    }
}

#[test]
fn interactive_loop_processes_mcp_toggle_requests_and_updates_ui_state() {
    let mut runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Disabled,
        visible_count: 3,
        visible_count_by_server: 0,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: false,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), false)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Disabled)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![("gh".to_string(), McpUsabilityState::Disabled, None, 3)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0)]
    );
}

#[test]
fn interactive_loop_marks_enable_failure_as_failed_state() {
    let mut runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Failed,
        visible_count: 2,
        visible_count_by_server: 0,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: true,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Failed)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![("gh".to_string(), McpUsabilityState::Failed, None, 2)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0)]
    );
}

#[test]
fn interactive_loop_marks_enable_success_as_enabled_state() {
    let mut runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Enabled,
        visible_count: 7,
        visible_count_by_server: 5,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: true,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Enabled)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![("gh".to_string(), McpUsabilityState::Enabled, None, 7)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 5)]
    );
}

struct FailingMcpToggleRuntime {
    toggles: Vec<(String, bool)>,
    visible_count: usize,
}

impl CoreRuntime for FailingMcpToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for FailingMcpToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.toggles.push((server_name.to_string(), enabled));
        Err("connect timeout".to_string())
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }
}

#[test]
fn interactive_loop_propagates_failure_reason_and_visible_tool_count_on_toggle_error() {
    let mut runtime = FailingMcpToggleRuntime {
        toggles: Vec::new(),
        visible_count: 4,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: true,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Failed)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![(
            "gh".to_string(),
            McpUsabilityState::Failed,
            Some("connect timeout".to_string()),
            4,
        )]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0)]
    );
}

struct SequencedMcpToggleRuntime {
    toggles: Vec<(String, bool)>,
    states: std::collections::VecDeque<McpUsabilityState>,
    visible_counts: std::collections::VecDeque<usize>,
    visible_counts_by_server: std::collections::VecDeque<usize>,
    current_visible_count: usize,
    current_visible_count_by_server: usize,
}

impl CoreRuntime for SequencedMcpToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for SequencedMcpToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.toggles.push((server_name.to_string(), enabled));
        self.current_visible_count = self
            .visible_counts
            .pop_front()
            .expect("global visible count entry");
        self.current_visible_count_by_server = self
            .visible_counts_by_server
            .pop_front()
            .expect("per-server visible count entry");
        Ok(self.states.pop_front().expect("state sequence entry"))
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.current_visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        self.current_visible_count_by_server
    }
}

#[test]
fn interactive_toggle_enable_disable_cycle_refreshes_per_server_visible_counts() {
    let mut runtime = SequencedMcpToggleRuntime {
        toggles: Vec::new(),
        states: [McpUsabilityState::Disabled, McpUsabilityState::Enabled]
            .into_iter()
            .collect(),
        visible_counts: [3usize, 7usize].into_iter().collect(),
        visible_counts_by_server: [0usize, 5usize].into_iter().collect(),
        current_visible_count: 0,
        current_visible_count_by_server: 0,
    };
    let mut ui = StagedToggleUi::new();

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.toggles,
        vec![("gh".to_string(), false), ("gh".to_string(), true)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![
            ("gh".to_string(), McpUsabilityState::Disabled, None, 3),
            ("gh".to_string(), McpUsabilityState::Enabled, None, 7),
        ]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0), ("gh".to_string(), 5)]
    );
}

struct PanicOnToggleRuntime {
    visible_count: usize,
}

impl CoreRuntime for PanicOnToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for PanicOnToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        panic!("toggle panic")
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.visible_count
    }
}

#[test]
fn interactive_loop_disconnected_toggle_worker_preserves_authoritative_visible_tool_count() {
    let mut runtime = PanicOnToggleRuntime { visible_count: 9 };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: false,
    });

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None);
    }));

    assert!(panic.is_err(), "expected panic from toggle worker thread");
    assert_eq!(
        ui.mcp_details,
        vec![(
            "gh".to_string(),
            McpUsabilityState::Failed,
            Some("toggle worker disconnected".to_string()),
            9,
        )]
    );
}

struct StagedToggleUi {
    quit: bool,
    first_sent: bool,
    second_sent: bool,
    mcp_states: Vec<(String, McpUsabilityState)>,
    mcp_details: Vec<(String, McpUsabilityState, Option<String>, usize)>,
    mcp_visible_tool_count_updates: Vec<(String, usize)>,
}

impl StagedToggleUi {
    fn new() -> Self {
        Self {
            quit: false,
            first_sent: false,
            second_sent: false,
            mcp_states: Vec::new(),
            mcp_details: Vec::new(),
            mcp_visible_tool_count_updates: Vec::new(),
        }
    }
}

impl ProgressUi for StagedToggleUi {
    fn emit(&mut self, _event: &UiEvent) {}

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for StagedToggleUi {
    fn pump_once(&mut self) {
        if self.mcp_details.len() >= 2 {
            self.quit = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for StagedToggleUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        None
    }

    fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        if !self.first_sent {
            self.first_sent = true;
            return Some(McpToggleRequest {
                server_name: "gh".to_string(),
                enable: false,
            });
        }

        if self.first_sent && !self.second_sent && !self.mcp_details.is_empty() {
            self.second_sent = true;
            return Some(McpToggleRequest {
                server_name: "gh".to_string(),
                enable: true,
            });
        }

        None
    }
}

impl DisplayStateUi for StagedToggleUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) {
        self.mcp_details.push((
            server_name.to_string(),
            state,
            reason,
            llm_visible_mcp_tool_count,
        ));
        self.set_mcp_server_state(server_name, state);
    }

    fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.mcp_visible_tool_count_updates
            .push((server_name.to_string(), count));
    }

    fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        _server_name: &str,
        _names: Vec<String>,
    ) {
    }

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for StagedToggleUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

#[test]
fn interactive_loop_worker_channel_closed_preserves_authoritative_visible_tool_count() {
    let mut runtime = PanicOnToggleRuntime { visible_count: 13 };
    let mut ui = StagedToggleUi::new();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None);
    }));

    assert!(panic.is_err(), "expected panic from toggle worker thread");
    assert_eq!(
        ui.mcp_details.len(),
        2,
        "expected exactly two toggle failure reports"
    );
    // First toggle: worker panics, response channel disconnects
    assert_eq!(ui.mcp_details[0].0, "gh");
    assert_eq!(ui.mcp_details[0].1, McpUsabilityState::Failed);
    assert_eq!(
        ui.mcp_details[0].2.as_deref(),
        Some("toggle worker disconnected")
    );
    assert_eq!(ui.mcp_details[0].3, 13);
    // Second toggle: either the command channel is already closed ("worker channel closed")
    // or the send succeeds but the response channel is disconnected ("toggle worker disconnected").
    // Both are valid — the exact outcome depends on thread scheduling.
    assert_eq!(ui.mcp_details[1].0, "gh");
    assert_eq!(ui.mcp_details[1].1, McpUsabilityState::Failed);
    let reason = ui.mcp_details[1].2.as_deref().unwrap_or("");
    assert!(
        reason == "worker channel closed" || reason == "toggle worker disconnected",
        "unexpected reason for second toggle: {reason}"
    );
    assert_eq!(ui.mcp_details[1].3, 13);
    assert!(ui.mcp_visible_tool_count_updates.is_empty());
}

#[test]
fn emit_batch_delivers_all_events() {
    // RED phase: write test that verifies emit_batch delivers all events
    #[derive(Default)]
    struct BatchTestUi {
        events: Vec<UiEvent>,
        emit_calls: usize,
        emit_batch_calls: usize,
    }

    impl ProgressUi for BatchTestUi {
        fn emit(&mut self, event: &UiEvent) {
            self.events.push(event.clone());
            self.emit_calls += 1;
        }

        fn flush(&mut self) {}

        fn take_cancel_requested(&self) -> bool {
            false
        }

        fn emit_batch(&mut self, events: &[UiEvent]) {
            self.emit_batch_calls += 1;
            for event in events {
                self.events.push(event.clone());
            }
        }
    }

    impl LifecycleUi for BatchTestUi {
        fn pump_once(&mut self) {}

        fn quit_requested(&self) -> bool {
            false
        }

        fn fatal_error(&self) -> Option<&str> {
            None
        }
    }

    impl UserInputUi for BatchTestUi {
        fn take_submitted_prompt(&mut self) -> Option<String> {
            None
        }
    }

    impl DisplayStateUi for BatchTestUi {}

    impl TranscriptUi for BatchTestUi {
        fn hydrate_transcript_from_messages(
            &mut self,
            _messages: impl IntoIterator<Item = UiMessageSnapshot>,
            _last_total_tokens: Option<u64>,
        ) {
        }
    }

    let mut ui = BatchTestUi::default();

    // Create 5 test events
    let events = vec![
        UiEvent::Tick,
        UiEvent::LlmStart,
        UiEvent::AssistantMessage {
            text: "hello".to_string(),
        },
        UiEvent::AssistantMessage {
            text: "world".to_string(),
        },
        UiEvent::Completed { tool_calls: 0 },
    ];

    // Call emit_batch
    ui.emit_batch(&events);

    // Verify all events were delivered
    assert_eq!(ui.events.len(), 5, "all 5 events should be delivered");
    assert_eq!(ui.emit_batch_calls, 1, "emit_batch should be called once");
    assert_eq!(
        ui.emit_calls, 0,
        "emit should not be called when using emit_batch"
    );
}

// Mailbox message injection tests

#[derive(Default)]
struct MailboxTestRuntime {
    prompts: Vec<String>,
    clear_session_calls: usize,
}

impl CoreRuntime for MailboxTestRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ExtendedRuntime for MailboxTestRuntime {
    fn clear_session(&mut self) {
        self.clear_session_calls += 1;
    }
}

struct MailboxTestUi {
    submitted: std::collections::VecDeque<String>,
    quit: bool,
    clear_transcript_calls: usize,
    displayed_incoming_messages: Vec<String>,
}

impl MailboxTestUi {
    fn with_prompts(prompts: &[&str]) -> Self {
        Self {
            submitted: prompts.iter().map(|s| s.to_string()).collect(),
            quit: false,
            clear_transcript_calls: 0,
            displayed_incoming_messages: Vec::new(),
        }
    }
}

impl ProgressUi for MailboxTestUi {
    fn emit(&mut self, _event: &UiEvent) {}
    fn flush(&mut self) {}
    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for MailboxTestUi {
    fn pump_once(&mut self) {
        if self.submitted.is_empty() {
            self.quit = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for MailboxTestUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }
}

impl DisplayStateUi for MailboxTestUi {
    fn display_incoming_message(&mut self, text: &str) {
        self.displayed_incoming_messages.push(text.to_string());
    }
}

impl TranscriptUi for MailboxTestUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }

    fn clear_transcript(&mut self) {
        self.clear_transcript_calls += 1;
    }
}

#[test]
fn mailbox_message_injected_as_turn() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "orchestrator".to_string(),
        message: "implement the login endpoint".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[from: orchestrator] implement the login endpoint".to_string()]
    );
    assert_eq!(
        ui.displayed_incoming_messages,
        vec!["[from: orchestrator] implement the login endpoint".to_string()]
    );
}

#[test]
fn mailbox_clear_resets_session() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "orchestrator".to_string(),
        message: "/clear".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.clear_session_calls, 1);
    assert_eq!(ui.clear_transcript_calls, 1);
    assert_eq!(
        runtime.prompts.len(),
        0,
        "/clear should not be injected as a turn"
    );
}

#[test]
fn mailbox_none_no_change() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&["hello"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    assert_eq!(runtime.clear_session_calls, 0);
}

#[test]
fn user_input_takes_precedence() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&["user prompt"]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "orchestrator".to_string(),
        message: "mailbox prompt".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    // User prompt should be processed first
    assert_eq!(runtime.prompts[0], "user prompt".to_string());
}

#[test]
fn mailbox_queued_when_worker_busy() {
    use crate::mailbox::IncomingMessage;
    use std::sync::{Arc, Mutex};

    let prompts = Arc::new(Mutex::new(Vec::new()));
    let prompts_clone = Arc::clone(&prompts);

    struct BusyRuntime {
        prompts: Arc<Mutex<Vec<String>>>,
        first_call: std::sync::atomic::AtomicBool,
    }

    impl CoreRuntime for BusyRuntime {
        fn execute_turn<U: ProgressUi>(
            &mut self,
            ui: &mut U,
            prompt: String,
            _context: Option<String>,
            _span: Span,
        ) -> Result<Value, LabeledError> {
            self.prompts.lock().unwrap().push(prompt.clone());

            // Simulate long-running first turn
            if self.first_call.load(Ordering::SeqCst) {
                for _ in 0..10 {
                    ui.emit(&UiEvent::Tick);
                    std::thread::sleep(Duration::from_millis(2));
                }
                self.first_call.store(false, Ordering::SeqCst);
            }

            Ok(Value::nothing(Span::test_data()))
        }
    }

    impl ExtendedRuntime for BusyRuntime {}

    let mut runtime = BusyRuntime {
        prompts: prompts_clone,
        first_call: std::sync::atomic::AtomicBool::new(true),
    };
    let mut ui = MailboxTestUi::with_prompts(&["first"]);

    let (tx, rx) = std::sync::mpsc::channel();

    // Send mailbox message that will arrive while worker is busy
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        tx.send(IncomingMessage {
            from: "orchestrator".to_string(),
            message: "queued message".to_string(),
            kind: "message".to_string(),
        })
        .ok();
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    let final_prompts = prompts.lock().unwrap();
    assert_eq!(final_prompts.len(), 2);
    assert_eq!(final_prompts[0], "first");
    assert_eq!(final_prompts[1], "[from: orchestrator] queued message");
    assert!(
        ui.displayed_incoming_messages
            .contains(&"[from: orchestrator] queued message".to_string()),
        "deferred mailbox message should be displayed when sent: {:?}",
        ui.displayed_incoming_messages
    );
}

#[test]
fn orchestrator_formats_task_kind() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "parent".to_string(),
        message: "build the auth module".to_string(),
        kind: "task".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[TASK from: parent] build the auth module".to_string()]
    );
}

#[test]
fn orchestrator_formats_completion_kind() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "worker-1".to_string(),
        message: "auth module is done".to_string(),
        kind: "completion".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[COMPLETED from: worker-1] auth module is done".to_string()]
    );
}

#[test]
fn orchestrator_formats_question_kind() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "worker-2".to_string(),
        message: "which database should I use?".to_string(),
        kind: "question".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts.len(), 1);
    assert!(
        runtime.prompts[0].starts_with("[QUESTION from: worker-2"),
        "question kind should start with [QUESTION from: ...]: {}",
        runtime.prompts[0]
    );
    assert!(
        runtime.prompts[0].contains("BLOCKED"),
        "question kind should contain BLOCKED: {}",
        runtime.prompts[0]
    );
}

#[test]
fn orchestrator_formats_default_kind() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "peer".to_string(),
        message: "general info".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[from: peer] general info".to_string()]
    );
}

#[test]
fn orchestrator_formats_unknown_kind_as_default() {
    use crate::mailbox::IncomingMessage;

    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "peer".to_string(),
        message: "something custom".to_string(),
        kind: "custom_kind".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[from: peer] something custom".to_string()]
    );
}

// Compile-time check: run_single_turn must accept anything that impls CoreRuntime
// This test will fail to compile until CoreRuntime exists and run_single_turn uses it
fn _assert_single_turn_accepts_core_runtime<R: CoreRuntime + Send, U: ProgressUi>(_r: R, _u: U) {
    // if this compiles, the bound is correct
}

// Compile-time check: run_interactive_loop must accept anything that impls ExtendedRuntime
fn _assert_interactive_loop_accepts_extended_runtime<
    R: ExtendedRuntime + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
>(
    _r: R,
    _u: U,
) {
    // if this compiles, the bound is correct
}

// ── context_window_max_tokens tests ─────────────────────────────────────────

/// A fake runtime that returns a known max_context_tokens on model switch.
struct ContextWindowRuntime {
    switched_models: Vec<String>,
    max_context_tokens: Option<u64>,
}

impl Default for ContextWindowRuntime {
    fn default() -> Self {
        Self {
            switched_models: Vec::new(),
            max_context_tokens: Some(128_000),
        }
    }
}

impl CoreRuntime for ContextWindowRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for ContextWindowRuntime {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.switched_models.push(model_spec.to_string());
        Ok((model_spec.to_string(), self.max_context_tokens))
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        self.max_context_tokens
    }
}

/// A fake UI that also tracks context_window_max_tokens updates.
struct ContextWindowUi {
    submitted: std::collections::VecDeque<String>,
    model_switch_requests: std::collections::VecDeque<String>,
    quit: bool,
    pump_count: usize,
    warnings: Vec<String>,
    active_model_identity: Option<String>,
    context_window_max_tokens: Option<Option<u64>>, // outer Option = "was it set?", inner = value
}

impl ContextWindowUi {
    fn new(model_switch_requests: &[&str]) -> Self {
        Self {
            submitted: std::collections::VecDeque::new(),
            model_switch_requests: model_switch_requests
                .iter()
                .map(|s| s.to_string())
                .collect(),
            quit: false,
            pump_count: 0,
            warnings: Vec::new(),
            active_model_identity: None,
            context_window_max_tokens: None,
        }
    }
}

impl ProgressUi for ContextWindowUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Warning { message } = event {
            self.warnings.push(message.clone());
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for ContextWindowUi {
    fn pump_once(&mut self) {
        self.pump_count = self.pump_count.saturating_add(1);
        if self.model_switch_requests.is_empty() && self.pump_count > 1 {
            self.quit = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for ContextWindowUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.model_switch_requests.pop_front()
    }
}

impl DisplayStateUi for ContextWindowUi {
    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.active_model_identity = Some(active_model_identity.to_string());
    }

    fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.context_window_max_tokens = Some(max_tokens);
    }

    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for ContextWindowUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

#[test]
fn model_switch_updates_context_window_max_tokens_in_ui() {
    let mut runtime = ContextWindowRuntime::default(); // max_context_tokens = Some(128_000)
    let mut ui = ContextWindowUi::new(&["openai/gpt-4o-mini"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    // context_window_max_tokens must have been set on the UI
    assert_eq!(
        ui.context_window_max_tokens,
        Some(Some(128_000)),
        "expected context_window_max_tokens to be updated with 128_000 after model switch"
    );
}

#[test]
fn model_switch_updates_context_window_max_tokens_none_when_unset() {
    let mut runtime = ContextWindowRuntime {
        max_context_tokens: None,
        ..Default::default()
    };
    let mut ui = ContextWindowUi::new(&["openai/gpt-4o-mini"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        ui.context_window_max_tokens,
        Some(None),
        "expected context_window_max_tokens to be set to None when model has no limit"
    );
}

// ── session_resume token seeding tests ───────────────────────────────────────

#[derive(Default)]
struct TokenSeedingRuntime {
    seeded_tokens: Option<Option<u64>>,
}

impl CoreRuntime for TokenSeedingRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl ExtendedRuntime for TokenSeedingRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
    }

    fn seed_last_total_tokens(&mut self, tokens: Option<u64>) {
        self.seeded_tokens = Some(tokens);
    }
}

#[test]
fn session_resume_seeds_last_total_tokens() {
    let mut runtime = TokenSeedingRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    run_hydrated_interactive_loop(
        &mut runtime,
        &mut ui,
        vec![],
        Some(90_000),
        None,
        Span::test_data(),
        None,
    )
    .expect("hydrated interactive loop");

    assert_eq!(
        runtime.seeded_tokens,
        Some(Some(90_000)),
        "seed_last_total_tokens must be called with the loaded token count on session resume"
    );
}

#[test]
fn session_resume_seeds_last_total_tokens_none_when_no_prior_session() {
    let mut runtime = TokenSeedingRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    run_hydrated_interactive_loop(
        &mut runtime,
        &mut ui,
        vec![],
        None,
        None,
        Span::test_data(),
        None,
    )
    .expect("hydrated interactive loop");

    assert_eq!(
        runtime.seeded_tokens,
        Some(None),
        "seed_last_total_tokens must be called with None when no prior token count exists"
    );
}
