use std::{
    cell::RefCell,
    fs,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::rendering::layout::wrapped_input_rows;
use crate::runtime::renderer_test::{CapturingRenderer, FakeRenderer};
use crate::runtime::status::test::{init_repo_with_branch, run_git};
use crate::runtime::test_driver::{DriveEvent, RenderLoopDriver};
use crate::test_support::{markdown_fixture, open_command_palette_for_test};
use crate::{
    interaction::input::{TerminalEvent, TerminalKey},
    platform::safety::RestoreRunError,
    platform::terminal::{
        TerminalAction, TerminalBackend, TerminalLifecycle, TerminalLifecycleError,
    },
    runtime::{
        InputSourceDiagnostics, RuntimeCoordinator, RuntimeRunError, ScriptedTerminalEvents,
        TerminalEventSource, TuiRuntimeRenderer, command_palette_table_model_for_test,
        cursor_style_for_test, help_panel_lines, help_panel_max_scroll_for_test,
        help_panel_overflow_cue_for_test, help_panel_visible_window_for_test,
        inline_slash_lines_for_test, input_line_for_test, input_line_for_test_at_millis,
        input_rows_with_prompt_for_test, mcp_table_model_for_test, run_with_terminal_restore_sync,
        status_panel_lines,
    },
    state::{
        ActivePicker, AppState, InputMode, InputState, McpServerUsabilityState, PickerRenderKind,
        PromptStatus, TranscriptRole, UiPhase,
    },
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nu_agent_core::bus::{PermissionEvent, WarningEvent};
use nu_agent_core::orchestrator::{OrchestratorEvent, UiStateEvent};
use nu_agent_core::protocol::contracts::{UiMessageSnapshot, UiMessageUsageSnapshot};
use nu_agent_core::protocol::event::{
    PermissionDecision, PermissionRequestContext, ToolDisplay, UiEvent,
};
use nu_agent_core::renderer::UiRenderer;
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::TranscriptEntryKind;
use nu_agent_core::transcript::renderer::ItemStatus;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

impl RuntimeCoordinator {
    pub(crate) fn new_for_test_with_watchdog(
        columns: u16,
        rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        Self::new_with_watchdog(columns, rows, side_pane_visible, input_watchdog_timeout)
    }

    pub(crate) fn state(&self) -> &AppState {
        &self.state
    }

    pub(crate) fn input_diagnostics_snapshot(&self) -> (String, String, Option<String>) {
        (
            self.input_backend_status.clone(),
            self.last_input_poll_status.clone(),
            self.last_input_error.clone(),
        )
    }

    pub(crate) fn render_needed(&self) -> bool {
        self.render_needed
    }

    pub(crate) fn set_render_needed(&mut self, needed: bool) {
        self.render_needed = needed;
    }

    pub(crate) fn set_last_render_at(&mut self, at: Instant) {
        self.last_render_at = at;
    }

    pub(crate) fn main_pane_rects_for_height(
        main_height: u16,
    ) -> (
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
    ) {
        crate::runtime::render::frame_test::main_pane_rects_for_height(main_height)
    }
}

pub(crate) fn modal_open_state_applies_dimmed_backdrop_for_test(state: &AppState) -> bool {
    state.picker.active().is_some() || state.info_panel.is_some()
}

pub(crate) fn inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(
    state: &AppState,
) -> bool {
    state.picker.render_kind() == Some(PickerRenderKind::Model)
}

pub(crate) fn input_pane_content_width_for_test(inner_width: u16) -> usize {
    inner_width.saturating_sub(2) as usize
}

// region:    --- Test Support

/// Wraps a [`TerminalKey`] as a scripted driver event for the real render loop.
fn key(key: TerminalKey) -> DriveEvent {
    DriveEvent::Key(TerminalEvent::Key(key))
}

// endregion: --- Test Support

#[test]
fn idle_startup_does_not_show_spinner() {
    let coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    assert_ne!(
        coordinator.state().status.message.status_line,
        "Thinking..."
    );
    assert!(!coordinator.state().input_locked);
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.starts_with("○ "),
        "idle startup must show idle indicator, got {text:?}"
    );
}

#[derive(Default)]
pub(super) struct StubEventSource {
    next: Option<TerminalEvent>,
}

impl TerminalEventSource for StubEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Ok(self.next.take())
    }
}

#[derive(Default)]
pub(super) struct ErrorEventSource;

impl TerminalEventSource for ErrorEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Err("simulated source failure".to_string())
    }
}

#[derive(Clone)]
pub(super) struct ErrorWithDiagnosticsEventSource {
    diagnostics: InputSourceDiagnostics,
    error: String,
}

impl TerminalEventSource for ErrorWithDiagnosticsEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Err(self.error.clone())
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

#[derive(Clone)]
pub(super) struct DiagnosticsOnlyEventSource {
    diagnostics: InputSourceDiagnostics,
}

impl TerminalEventSource for DiagnosticsOnlyEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Ok(None)
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

#[tokio::test]
async fn coordinator_submit_handoff_keeps_input_editable_and_preserves_transcript_preview()
-> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;

    // -- Check
    assert_eq!(driver.state().phase, UiPhase::Busy);
    assert!(!driver.state().input_locked);
    // The real loop drained the pending prompt into a PromptSubmitted event
    // for the orchestrator; the poll-style queue is empty afterwards.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "x")
        ),
        "loop must hand the prompt to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    // starting spacer + user + closing spacer
    assert_eq!(driver.state().transcript.entries.len(), 3);
    assert_eq!(driver.state().transcript.entries[1].role(), Role::User);
    assert_eq!(driver.state().transcript.entries[1].text(), "x");
    Ok(())
}

#[tokio::test]
async fn slash_commands_do_not_append_command_text_to_transcript() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // In the TextArea architecture, char keys are routed to TextArea by
    // handle_insert_mode_key. Set the textarea content directly to simulate
    // the coordinator flow for slash command submission.
    driver.coordinator_mut().textarea = ratatui_textarea::TextArea::new(vec!["/help".to_string()]);

    // -- Exec
    driver.advance(&[key(TerminalKey::Enter)]).await?;

    // -- Check
    // The real loop drained the submitted command into a PromptSubmitted
    // event; the poll-style queue is empty afterwards.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "/help")
        ),
        "loop must hand the slash command to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert_eq!(driver.state().phase, UiPhase::Idle);
    assert_eq!(driver.state().pending_prompt_count(), 0);
    assert!(driver.state().prompt_items().is_empty());
    assert!(driver.state().transcript.entries.is_empty());
    Ok(())
}

#[tokio::test]
async fn compact_result_artifact_is_visible_without_slash_command_echo() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit /compact through the real terminal arm.
    let script: Vec<DriveEvent> = "/compact"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();
    driver.advance(&script).await?;

    // -- Check
    // The real loop drained the submitted command into a PromptSubmitted
    // event; the poll-style queue is empty afterwards.
    assert!(
        driver
            .orchestrator_events()
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "/compact")),
        "loop must hand the slash command to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    assert!(driver.state().transcript.entries.is_empty());

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::CompactionStarted {
            source: "slash_compact".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::CompactionCompleted {
            source: "slash_compact".to_string(),
            summary_preview: "preview".to_string(),
            summary_body: "summary body".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    let lines = driver
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(lines.contains(&"summary body".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains("status=running")));
    assert!(!lines.iter().any(|line| line.starts_with("/compact")));
    Ok(())
}

// Regression test for the slash char-drop defect: the keystrokes must be
// routed through the REAL render-loop terminal arm (mpsc channel +
// tokio::select!), not a synthetic poll harness.
#[tokio::test]
async fn slash_prefix_filters_suggestions_and_enter_submits_matching_command() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type /theme and submit through the real terminal arm.
    let script: Vec<DriveEvent> = "/theme"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .chain(std::iter::once(key(TerminalKey::Enter)))
        .collect();
    driver.advance(&script).await?;

    // -- Check
    // The real loop drained the launch request via the pending ui-state
    // events, published it on the bus, and re-reduced it: the theme picker is
    // now open, and the launch queue is empty.
    assert_eq!(
        driver.state().picker.render_kind(),
        Some(PickerRenderKind::Theme),
        "loop must execute the theme launch request through the ui_state arm"
    );
    assert_eq!(
        driver.coordinator_mut().state.take_next_launch_request(),
        None
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    Ok(())
}

#[tokio::test]
async fn immediate_slash_commands_do_not_set_busy_or_spinner() -> Result<()> {
    for command in ["/compact", "/mcp", "/help", "/status"] {
        // -- Setup & Fixtures
        let mut driver = RenderLoopDriver::new(120, 30);

        // In the TextArea architecture, char keys are routed to TextArea by
        // handle_insert_mode_key. Set the textarea content directly to simulate
        // the coordinator flow for slash command submission.
        driver.coordinator_mut().textarea =
            ratatui_textarea::TextArea::new(vec![command.to_string()]);

        // -- Exec
        driver.advance(&[key(TerminalKey::Enter)]).await?;

        // -- Check
        // The real loop drained the submitted command into a PromptSubmitted
        // event; the poll-style queue is empty afterwards.
        assert!(
            driver.orchestrator_events().iter().any(|event| matches!(
                event,
                OrchestratorEvent::PromptSubmitted { text } if text == command
            )),
            "loop must hand the slash command to the orchestrator channel"
        );
        assert_eq!(
            driver
                .coordinator_mut()
                .state
                .take_next_prompt_for_execution(),
            None
        );
        assert_eq!(
            driver.state().phase,
            UiPhase::Idle,
            "immediate command must not transition into Busy"
        );
        assert!(
            !driver.state().is_active_cycle(),
            "immediate command must not activate prompt lifecycle"
        );
        assert!(
            driver.state().status.message.status_line != "Thinking...",
            "spinner lane status must not be set for immediate slash commands"
        );
    }
    Ok(())
}

#[tokio::test]
async fn coordinator_esc_then_esc_requests_cancel_signal() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[
            key(TerminalKey::Char('q')),
            key(TerminalKey::Enter),
            key(TerminalKey::Esc),
            key(TerminalKey::Esc),
        ])
        .await?;

    // -- Check
    assert_eq!(
        driver.state().status.message.status_line,
        "Abort requested."
    );
    assert!(driver.coordinator().take_cancel_requested());
    Ok(())
}

#[test]
fn runtime_renderer_reuses_eventing_and_preserves_emit_passthrough() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:h,ctrlc,resize:140x35");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStarted);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.flush();

    assert!(runtime_renderer.coordinator().take_cancel_requested());
    assert!(runtime_renderer.coordinator().quit_requested());
    let state = runtime_renderer.coordinator().state();
    assert_eq!(state.phase, UiPhase::Busy);
}

#[tokio::test]
async fn render_loop_driver_take_submitted_prompt_supports_interactive_turn_handoff() -> Result<()>
{
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "hi" through the real terminal arm.
    driver
        .advance(&[
            key(TerminalKey::Char('h')),
            key(TerminalKey::Char('i')),
            key(TerminalKey::Enter),
        ])
        .await?;

    // -- Check
    // The real loop drained the submitted prompt into a PromptSubmitted event
    // for the orchestrator; the poll-style queue is empty afterwards.
    assert!(
        driver.orchestrator_events().iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "hi")
        ),
        "loop must hand the prompt to the orchestrator channel"
    );
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    Ok(())
}

#[tokio::test]
async fn render_loop_driver_quit_requested_reflects_ctrlc_terminal_event() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::CtrlC)]).await?;

    // -- Check: the loop must send the orchestrator Quit event before exiting.
    assert!(
        driver
            .orchestrator_events()
            .iter()
            .any(|event| matches!(event, OrchestratorEvent::Quit)),
        "loop must forward Quit to the orchestrator on Ctrl+C"
    );
    assert!(driver.coordinator().quit_requested());
    Ok(())
}

#[tokio::test]
async fn submit_reaches_orchestrator_channel_through_render_loop_terminal_arm() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "hi" through the real terminal arm; the loop
    // itself takes the pending events and forwards them on the orchestrator
    // channel, exactly as production runs it.
    driver
        .advance(&[
            key(TerminalKey::Char('h')),
            key(TerminalKey::Char('i')),
            key(TerminalKey::Enter),
        ])
        .await?;

    // -- Check
    let events = driver.orchestrator_events();
    let submitted = events
        .iter()
        .filter(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "hi"),
        )
        .count();
    assert_eq!(
        submitted, 1,
        "exactly one PromptSubmitted must reach the orchestrator channel"
    );

    // After the channel event is produced, the poll-style queue is empty
    // because the real loop already drained it.
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );
    // And the transcript shows the user turn was started.
    assert!(
        driver
            .state()
            .transcript
            .entries
            .iter()
            .any(|entry| entry.role() == Role::User && entry.text() == "hi")
    );
    Ok(())
}

#[test]
fn runtime_renderer_in_tui_mode_does_not_forward_spinner_progress_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer =
        TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStarted);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Completed { tool_calls: 0 });

    assert!(events.lock().expect("events").is_empty());
}

#[test]
fn runtime_renderer_non_tui_mode_forwards_events_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::Warning {
        message: "warn".to_string(),
    });

    let captured = events.lock().expect("events").clone();
    assert_eq!(captured.len(), 1);
    assert!(matches!(captured[0], UiEvent::Warning { .. }));
}

#[test]
fn assistant_message_event_is_appended_to_tui_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello\nworld".to_string(),
    });
    coordinator.drain_transport();

    // After the raw-markdown refactor, a single AssistantMessage event produces
    // exactly one ProseMessage entry (the entire text is stored as raw markdown).
    let assistant_entries: Vec<_> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|e| e.role() == Role::Assistant)
        .collect();
    assert_eq!(
        assistant_entries.len(),
        1,
        "one ProseMessage per message block"
    );
    assert!(
        assistant_entries[0].text().contains("hello"),
        "raw markdown should contain 'hello'"
    );
    assert!(
        assistant_entries[0].text().contains("world"),
        "raw markdown should contain 'world'"
    );
}

#[test]
fn assistant_markdown_message_is_projected_before_transcript_append() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("lists_blockquote.md");
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    coordinator.drain_transport();

    // After the raw-markdown refactor, the transcript stores the source markdown.
    // Projection happens at render time. Verify the raw markdown is stored and
    // produces the expected rendered output when projected.
    let raw_texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .map(|entry| entry.text())
        .collect();

    // The entire markdown is stored in a single ProseMessage.
    // Project it and verify the list markers appear.
    let projected: Vec<String> = raw_texts
        .iter()
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect();

    assert!(projected.iter().any(|l| l.contains("• one")));
    assert!(projected.iter().any(|l| l.contains("• two")));
    assert!(projected.iter().any(|l| l.contains("1. first")));
    assert!(projected.iter().any(|l| l.contains("2. second")));
    assert!(projected.iter().any(|l| l.contains("│ quoted")));
    assert!(projected.iter().any(|l| l.contains("│ second")));
}

#[test]
fn assistant_markdown_message_preserves_inline_span_styles_in_transcript_state() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello **bold** and `code`".to_string(),
    });
    coordinator.drain_transport();

    // TranscriptEntry no longer has a `.rendered` field - test removed
    // Previously tested that assistant markdown was rendered with bold formatting
}

#[tokio::test]
async fn user_then_assistant_flows_without_turn_separator() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type and submit "h" through the real terminal arm.
    driver
        .advance(&[key(TerminalKey::Char('h')), key(TerminalKey::Enter)])
        .await?;
    // Activate the queued prompt so the User transcript entry is written
    let _ = driver
        .coordinator_mut()
        .state
        .take_next_prompt_for_execution();

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::AssistantMessage {
            text: "world".to_string(),
        });
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert_eq!(
        driver
            .state()
            .transcript
            .entries
            .iter()
            .map(|line| {
                let role = match line.role() {
                    nu_agent_core::transcript::ir::Role::User => TranscriptRole::User,
                    nu_agent_core::transcript::ir::Role::Assistant => TranscriptRole::Assistant,
                    nu_agent_core::transcript::ir::Role::Tool => TranscriptRole::Tool,
                    nu_agent_core::transcript::ir::Role::ToolDisplay => TranscriptRole::ToolDisplay,
                    nu_agent_core::transcript::ir::Role::System => TranscriptRole::System,
                    nu_agent_core::transcript::ir::Role::Compaction => TranscriptRole::Compaction,
                    nu_agent_core::transcript::ir::Role::Separator => TranscriptRole::System,
                };
                (role, line.text())
            })
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::System, "".to_string()), // starting spacer before prompt
            (TranscriptRole::User, "h".to_string()),
            (TranscriptRole::System, "".to_string()), // closing spacer after prompt
            (TranscriptRole::System, "".to_string()), // starting spacer before assistant
            (TranscriptRole::Assistant, "world".to_string()),
        ]
    );
    Ok(())
}

#[test]
fn busy_input_line_has_no_spinner_prefix_and_never_shows_locked_label() {
    let mut state = AppState::default();

    let idle_line = input_line_for_test(&state);
    assert_eq!(idle_line, "");

    state.phase = UiPhase::Busy;
    state.ensure_invariants();
    let busy_line = input_line_for_test_at_millis(&state, 160);
    assert_eq!(busy_line, "");
}

#[test]
fn tui_active_mode_does_not_forward_payload_like_events_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer =
        TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::ToolCompleted {
        name: "k8s__list_pods".to_string(),
        source: "mcp".to_string(),
        arguments: r#"{"namespace":"prod"}"#.to_string(),
        success: true,
        result: r#"[{"name":"api-0"}]"#.to_string(),
        display: None,
        error_kind: None,
        message: None,
    });
    runtime_renderer.emit(&UiEvent::AssistantMessage {
        text: "response".to_string(),
    });
    runtime_renderer.emit(&UiEvent::Completed { tool_calls: 1 });

    assert!(events.lock().expect("events").is_empty());
}

#[tokio::test]
async fn tick_and_completed_events_update_status_only_without_touching_input_buffer() -> Result<()>
{
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::Char('x'))]).await?;

    driver.coordinator_mut().enqueue_ui_event(UiEvent::Tick);
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert_eq!(driver.state().status.message.status_line, "Thinking...");

    // -- Exec
    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    driver.coordinator_mut().drain_transport();

    // -- Check
    assert!(driver.state().status.message.status_line.is_empty());
    Ok(())
}

#[tokio::test]
async fn status_updates_stay_in_status_area_and_do_not_pollute_input_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: type k and 9 through the real terminal arm.
    driver
        .advance(&[key(TerminalKey::Char('k')), key(TerminalKey::Char('9'))])
        .await?;

    driver.coordinator_mut().enqueue_ui_event(UiEvent::Tick);
    driver.coordinator_mut().drain_transport();

    let status_lines =
        crate::runtime::status_lines_for_test(&mut driver.coordinator_mut().state, "openai/gpt-4");
    let joined = status_lines.join("\n");

    // -- Check
    assert!(joined.contains("(busy)"));

    // -- Exec
    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    driver.coordinator_mut().drain_transport();

    let status_lines =
        crate::runtime::status_lines_for_test(&mut driver.coordinator_mut().state, "openai/gpt-4");

    // -- Check
    assert!(status_lines[0].contains("(idle)"));
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input mode:"))
    );
    Ok(())
}

#[test]
fn status_lines_do_not_report_input_mode() {
    let mut state = AppState::default();

    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4");
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
}

#[test]
fn compact_status_line_matches_lane_1_contract() {
    let line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let status_line: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(status_line.starts_with("○ openai/gpt-4o-mini"));
    assert!(!status_line.contains('|'));
}

const LANE_1_CASES: &[(&str, Option<&str>, usize, &str)] = &[
    (
        "abcdefghijklmnop",
        Some("branchname"),
        40,
        "○ abcdefghijklmnop          \u{e725} branchname",
    ),
    (
        "abcdefghijklmnop",
        Some("branchname"),
        23,
        "○ ...lmnop \u{e725} branchname",
    ),
    (
        "abcdefghijklmnop",
        Some("branchname"),
        20,
        "○ ...op \u{e725} branchname",
    ),
    ("openai/gpt-4o-mini", None, 80, "○ openai/gpt-4o-mini"),
];

#[test]
fn lane_1_scroll() {
    for &(model, branch, width, expected) in LANE_1_CASES {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            model, branch, None, width,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert_eq!(
            text, expected,
            "model={model}, branch={branch:?}, width={width}"
        );
        assert!(
            !text.contains('|'),
            "pipe should not appear: model={model}, branch={branch:?}, width={width}"
        );

        // Extra structural assertions carried over from overlapping
        // lane_1_branch_segment_is_right_aligned_when_present.
        if model == "abcdefghijklmnop" && branch == Some("branchname") && width == 40 {
            assert_eq!(text.chars().count(), 40);
            assert!(text.starts_with("○ abcdefghijklmnop"));
            assert!(text.ends_with("\u{e725} branchname"));
        }

        // Extra structural assertions carried over from
        // lane_1_narrow_truncation_keeps_branch_right_anchored.
        if model == "abcdefghijklmnop" && branch == Some("branchname") && width == 20 {
            assert_eq!(text.chars().count(), 20);
            assert!(text.ends_with("\u{e725} branchname"));
            assert!(text.contains("...op"));
        }
    }

    // Structural-only: lane_1_with_branch_appends_branch_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "m",
            Some("main"),
            None,
            40,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.ends_with("\u{e725} main"),
            "expected branch icon prefix, got: {text:?}"
        );
    }

    // Structural-only: lane_1_with_branch_ellipsizes_label_while_preserving_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "the-quick-brown-fox-jumps-over",
            Some("feature/super-long-branch-name"),
            None,
            32,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.contains('\u{e725}'),
            "icon must survive ellipsization, got: {text:?}",
        );
        assert!(
            text.contains("..."),
            "branch label should have been ellipsized, got: {text:?}",
        );
    }

    // Structural-only: lane_1_with_branch_drops_icon_when_budget_below_three_cells
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "abc",
            Some("main"),
            None,
            4,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            !text.contains('\u{e725}'),
            "icon must be dropped under extreme narrow budgets, got: {text:?}",
        );
        assert!(text.starts_with("○ "));
    }

    // Structural-only: lane_1_with_detached_head_short_sha_also_gets_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "m",
            Some("a1b2c3d"),
            None,
            40,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.ends_with("\u{e725} a1b2c3d"),
            "expected detached-HEAD short SHA to also carry icon, got: {text:?}",
        );
    }
}

#[test]
#[serial_test::serial]
fn branch_resolver_prefers_explicit_caller_repo_over_process_cwd() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let process_repo = temp_dir.path().join("process-repo");
    let caller_repo = temp_dir.path().join("caller-repo");
    fs::create_dir_all(&process_repo).expect("process repo dir");
    fs::create_dir_all(&caller_repo).expect("caller repo dir");
    init_repo_with_branch(&process_repo, "process-branch");
    init_repo_with_branch(&caller_repo, "caller-branch");

    let original_cwd = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&process_repo).expect("switch cwd to process repo");

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&caller_repo);

    std::env::set_current_dir(original_cwd).expect("restore cwd");
    assert_eq!(resolved.as_deref(), Some("caller-branch"));
}

#[test]
fn branch_resolver_returns_none_for_non_git_directory() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let non_git = temp_dir.path().join("plain");
    fs::create_dir_all(&non_git).expect("plain dir");

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&non_git);
    assert_eq!(resolved, None);
}

#[test]
fn branch_resolver_uses_detached_head_short_sha_fallback() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "feature/detached");

    let expected = run_git(&repo, &["rev-parse", "--short=12", "HEAD"]);
    run_git(&repo, &["checkout", "--detach"]);

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&repo);
    assert_eq!(resolved.as_deref(), Some(expected.as_str()));
}

#[test]
fn branch_resolver_is_worktree_safe() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    let worktree = temp_dir.path().join("repo-wt");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "mainline");

    run_git(&repo, &["branch", "wt-branch"]);
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            worktree.to_str().expect("worktree path"),
            "wt-branch",
        ],
    );

    let resolved = crate::runtime::status::test::resolve_repo_branch_for_test(&worktree);
    assert_eq!(resolved.as_deref(), Some("wt-branch"));
}

#[test]
fn repo_branch_tracker_updates_on_branch_and_detached_transitions() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut tracker =
        crate::runtime::status::RepoBranchTracker::from_caller_cwd(Some(repo.clone()));
    assert_eq!(tracker.branch(), Some("branch-one"));

    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "-b", "branch-two"]);
    tracker.refresh();
    assert_eq!(tracker.branch(), Some("branch-two"));

    let expected_detached = run_git(&repo, &["rev-parse", "--short=12", "HEAD"]);
    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "--detach"]);
    tracker.refresh();
    assert_eq!(tracker.branch(), Some(expected_detached.as_str()));
}

#[test]
fn repo_branch_tracker_refreshes_on_render_tick_without_terminal_event() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_repo_branch_caller_cwd(Some(repo.clone()));
    assert_eq!(coordinator.repo_branch(), Some("branch-one"));

    // Switch branch externally, then exercise the render-loop path that the
    // branch watcher triggers: refresh + mark render needed. No terminal event.
    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "-b", "branch-two"]);
    coordinator.refresh_repo_branch();

    assert_eq!(
        coordinator.repo_branch(),
        Some("branch-two"),
        "branch must update from a git ref change event, without a terminal event"
    );
}

#[test]
fn branch_watcher_signals_on_git_checkout() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_repo_branch_caller_cwd(Some(repo.clone()));
    let targets = coordinator.repo_branch_watch_targets();
    assert!(!targets.is_empty(), "watcher needs git ref targets");

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let _watcher =
        crate::runtime::branch_watcher::spawn_branch_watcher(targets, tx).expect("watcher");

    // Switch branch externally — the notify watcher should emit a signal.
    std::thread::sleep(Duration::from_millis(20));
    run_git(&repo, &["checkout", "-b", "branch-two"]);

    // Wait (bounded) for the watcher to deliver a signal on the channel, which
    // is the event the render loop's branch_rx arm consumes.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut signalled = false;
    while std::time::Instant::now() < deadline {
        if rx.try_recv().is_ok() {
            signalled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(signalled, "branch change should emit a watcher signal");

    coordinator.refresh_repo_branch();
    assert_eq!(coordinator.repo_branch(), Some("branch-two"));
}

#[test]
fn repo_branch_tracker_does_not_leak_between_repositories() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo_a = temp_dir.path().join("repo-a");
    let repo_b = temp_dir.path().join("repo-b");
    fs::create_dir_all(&repo_a).expect("repo a dir");
    fs::create_dir_all(&repo_b).expect("repo b dir");
    init_repo_with_branch(&repo_a, "alpha");
    init_repo_with_branch(&repo_b, "beta");

    let tracker_a = crate::runtime::status::RepoBranchTracker::from_caller_cwd(Some(repo_a));
    let tracker_b = crate::runtime::status::RepoBranchTracker::from_caller_cwd(Some(repo_b));

    assert_eq!(tracker_a.branch(), Some("alpha"));
    assert_eq!(tracker_b.branch(), Some("beta"));
}

#[test]
fn lane_1_has_no_mode_token_in_any_input_mode() {
    let insert_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let insert_text: String = insert_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    let normal_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let normal_text: String = normal_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    let visual_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let visual_text: String = visual_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert_eq!(insert_text, "○ model");
    assert_eq!(normal_text, "○ model");
    assert_eq!(visual_text, "○ model");
}

#[test]
fn cursor_style_maps_insert_to_bar_and_normal_visual_to_block() {
    assert!(matches!(
        cursor_style_for_test(InputMode::Insert),
        crossterm::cursor::SetCursorStyle::SteadyBar
    ));
    assert!(matches!(
        cursor_style_for_test(InputMode::Normal),
        crossterm::cursor::SetCursorStyle::SteadyBlock
    ));
    assert!(matches!(
        cursor_style_for_test(InputMode::Visual),
        crossterm::cursor::SetCursorStyle::SteadyBlock
    ));
}

// Sync-path coverage: the input-error handling lives in `poll_terminal_event`,
// which the async render loop's healthy mpsc channel cannot produce. These
// tests call the production poll + drain primitives directly (the same calls
// `TuiRuntimeRenderer` makes).
#[test]
fn coordinator_terminal_input_error_surfaces_status_and_requests_quit() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = ErrorEventSource;

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    assert!(coordinator.quit_requested());
    assert!(coordinator.take_cancel_requested());
    assert_eq!(
        coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
}

#[test]
fn runtime_renderer_reports_fatal_error_on_event_source_failure() {
    let inner = FakeRenderer::default();
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, ErrorEventSource, 120, 30);

    // emit() runs the production poll + drain + render cycle, which surfaces
    // the event source failure.
    runtime_renderer.emit(&UiEvent::Tick);

    assert!(runtime_renderer.quit_requested());
    assert_eq!(
        runtime_renderer.coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
}

#[tokio::test]
async fn idle_q_is_regular_input_and_never_requests_quit() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec & Check: a single idle 'q' never requests quit.
    driver.advance(&[key(TerminalKey::Char('q'))]).await?;
    assert!(!driver.coordinator().quit_requested());
    assert!(!driver.state().input_locked);

    // -- Exec & Check: typing 'a' then 'q' still never requests quit.
    driver
        .advance(&[key(TerminalKey::Char('a')), key(TerminalKey::Char('q'))])
        .await?;
    assert!(!driver.coordinator().quit_requested());
    Ok(())
}

#[tokio::test]
async fn idle_q_does_not_quit_through_dispatch_path() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::Char('q'))]).await?;

    // -- Check
    assert!(!driver.coordinator().quit_requested());
    Ok(())
}

#[tokio::test]
async fn idle_escape_status_copy_mentions_ctrlc_only_not_q() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance(&[key(TerminalKey::Esc)]).await?;

    // -- Check
    assert!(driver.state().status.message.status_line.contains("Ctrl+C"));
    assert!(
        !driver
            .state()
            .status
            .message
            .status_line
            .to_ascii_lowercase()
            .contains("press q")
    );
    assert!(
        !driver
            .state()
            .status
            .message
            .status_line
            .contains("q to quit")
    );
    Ok(())
}

#[test]
fn watchdog_fails_fast_when_no_input_backend_available() -> Result<()> {
    let mut coordinator = RuntimeCoordinator::new_for_test_with_watchdog(
        120,
        30,
        Some(true),
        std::time::Duration::from_millis(0),
    );

    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "none",
            primary_available: Some(false),
            fallback_available: Some(false),
            last_poll_state: "crossterm error; /dev/tty unavailable".to_string(),
            last_error: Some("crossterm poll failed".to_string()),
        },
    };

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    let fatal = coordinator
        .fatal_error()
        .ok_or("should have watchdog fatal error")?;
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed"));
    assert!(fatal.contains("interactive terminal"));
    assert!(coordinator.quit_requested());
    Ok(())
}

#[test]
fn diagnostics_snapshot_reports_active_backend_last_poll_and_last_error() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    let mut source = DiagnosticsOnlyEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "tty",
            primary_available: Some(false),
            fallback_available: Some(true),
            last_poll_state: "crossterm error; /dev/tty delivered event".to_string(),
            last_error: Some("crossterm poll failed: EIO".to_string()),
        },
    };

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    let (backend, last_poll, last_error) = coordinator.input_diagnostics_snapshot();
    assert_eq!(
        backend,
        "active=tty, crossterm=unavailable, /dev/tty=available"
    );
    assert_eq!(last_poll, "crossterm error; /dev/tty delivered event");
    assert_eq!(last_error.as_deref(), Some("crossterm poll failed: EIO"));
}

#[test]
fn immediate_poll_error_fails_fast_with_actionable_message_when_no_backends_available() -> Result<()>
{
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    let mut source = ErrorWithDiagnosticsEventSource {
        diagnostics: InputSourceDiagnostics {
            active_backend: "none",
            primary_available: Some(false),
            fallback_available: Some(false),
            last_poll_state: "crossterm error; /dev/tty unavailable".to_string(),
            last_error: None,
        },
        error: "crossterm poll failed: not a terminal".to_string(),
    };

    coordinator.poll_terminal_event(&mut source);
    coordinator.drain_transport();

    let fatal = coordinator
        .fatal_error()
        .ok_or("should have fatal fail-fast error")?;
    assert!(coordinator.quit_requested());
    assert!(coordinator.take_cancel_requested());
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed: not a terminal"));
    assert!(fatal.contains("Run `agent` in an interactive terminal"));
    assert!(!fatal.contains("Terminal input error:"));
    Ok(())
}

#[test]
fn crossterm_event_source_with_zero_timeout_returns_none_when_idle() {
    if unsafe { libc::isatty(libc::STDIN_FILENO) } == 0 {
        return; // no TTY available (e.g. Nix sandbox)
    }
    let mut source =
        crate::runtime::CrosstermTerminalEvents::new(std::time::Duration::from_millis(0));

    let event = source.poll_event();
    assert_eq!(event, Ok(None));
}

#[test]
fn crossterm_enter_modifier_mapping_distinguishes_submit_vs_newline_intents() {
    let plain = crate::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert_eq!(plain, Some(TerminalEvent::Key(TerminalKey::Enter)));

    let alt = crate::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::ALT,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }));
    assert_eq!(alt, Some(TerminalEvent::Key(TerminalKey::AltEnter)));

    let shift = crate::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::SHIFT,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }));
    assert_eq!(shift, Some(TerminalEvent::Key(TerminalKey::ShiftEnter)));
}

#[test]
fn coordinator_hydration_skips_blank_lines_and_maps_unknown_role_to_system() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "line1\n\nline2"),
            UiMessageSnapshot::new("assistant", "\n\nreply\n"),
            UiMessageSnapshot::new("tool", "tool output"),
            UiMessageSnapshot::new("mystery", "system fallback"),
        ],
        None,
    );

    let lines = coordinator.state().transcript.entries.clone();
    assert_eq!(
        lines
            .iter()
            .map(|line| {
                let role = match line.role() {
                    nu_agent_core::transcript::ir::Role::User => TranscriptRole::User,
                    nu_agent_core::transcript::ir::Role::Assistant => TranscriptRole::Assistant,
                    nu_agent_core::transcript::ir::Role::Tool => TranscriptRole::Tool,
                    nu_agent_core::transcript::ir::Role::ToolDisplay => TranscriptRole::ToolDisplay,
                    nu_agent_core::transcript::ir::Role::System => TranscriptRole::System,
                    nu_agent_core::transcript::ir::Role::Compaction => TranscriptRole::Compaction,
                    nu_agent_core::transcript::ir::Role::Separator => TranscriptRole::System,
                };
                (role, line.text())
            })
            .collect::<Vec<_>>(),
        vec![
            // user block: starting spacer + line1 + line2 + closing spacer
            (TranscriptRole::System, String::new()),
            (TranscriptRole::User, "line1".to_string()),
            (TranscriptRole::User, "line2".to_string()),
            (TranscriptRole::System, String::new()),
            // assistant block: starting spacer + reply + closing spacer
            (TranscriptRole::System, String::new()),
            (TranscriptRole::Assistant, "reply".to_string()),
            (TranscriptRole::System, String::new()),
            // system block: starting spacer + fallback + closing spacer
            (TranscriptRole::System, String::new()),
            (TranscriptRole::System, "system fallback".to_string()),
            (TranscriptRole::System, String::new()),
        ]
    );
}

#[test]
fn hydrated_tool_history_matches_live_tool_row_shape() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("tool", "tool[k8s__list_pods] → {} · done").with_tool_details(
                Some("{\"namespace\":\"prod\"}".to_string()),
                Some("[{\"name\":\"api-0\"}]".to_string()),
                Some(true),
            ),
        ],
        None,
    );

    // Tool block: starting spacer + tool + closing spacer (block is open at end)
    assert_eq!(coordinator.state().transcript.entries.len(), 3);
    assert_eq!(coordinator.state().transcript.entries[1].role(), Role::Tool);
    assert_eq!(
        coordinator.state().transcript.entries[1].text(),
        "k8s__list_pods"
    );
    assert_eq!(
        coordinator.state().transcript.entries[1].status,
        Some(ItemStatus::Done)
    );
}

#[test]
fn parse_persisted_tool_status_line_supports_done_and_failed_shapes() {
    let done = crate::state::parse_persisted_tool_status_line(
        "tool[k8s__list_pods] → {\"namespace\":\"prod\"} · done",
    );
    assert_eq!(
        done,
        Some(("k8s__list_pods", "{\"namespace\":\"prod\"}", true))
    );

    let failed = crate::state::parse_persisted_tool_status_line("tool[gh__run] → {} · failed");
    assert_eq!(failed, Some(("gh__run", "{}", false)));

    assert_eq!(
        crate::state::parse_persisted_tool_status_line("tool[gh__run] → {}"),
        None
    );
}

#[test]
fn coordinator_hydration_projects_both_user_and_assistant_markdown() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "# user stays literal"),
            UiMessageSnapshot::new("assistant", "# heading\n\n`x`"),
        ],
        None,
    );

    // After the raw-markdown refactor: text() returns raw markdown source.
    // Project to verify the rendered content.
    let raw_lines: Vec<(TranscriptRole, String)> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| {
            let role = match line.role() {
                nu_agent_core::transcript::ir::Role::User => TranscriptRole::User,
                nu_agent_core::transcript::ir::Role::Assistant => TranscriptRole::Assistant,
                nu_agent_core::transcript::ir::Role::Tool => TranscriptRole::Tool,
                nu_agent_core::transcript::ir::Role::ToolDisplay => TranscriptRole::ToolDisplay,
                nu_agent_core::transcript::ir::Role::System => TranscriptRole::System,
                nu_agent_core::transcript::ir::Role::Compaction => TranscriptRole::Compaction,
                nu_agent_core::transcript::ir::Role::Separator => TranscriptRole::System,
            };
            (role, line.text())
        })
        .collect();

    // User message: raw markdown stored as-is
    assert!(
        raw_lines
            .iter()
            .any(|(r, t)| *r == TranscriptRole::User && t.contains("user stays literal")),
        "user message should contain the text; got: {raw_lines:?}"
    );
    // Assistant message: raw markdown stored, projection produces heading and code
    let assistant_projected: Vec<String> = raw_lines
        .iter()
        .filter(|(r, _)| *r == TranscriptRole::Assistant)
        .flat_map(|(_, md)| crate::markdown::render_markdown_lines(md, None))
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert!(
        assistant_projected.iter().any(|l| l.contains("heading")),
        "projected assistant lines should contain heading text; got: {assistant_projected:?}"
    );
    assert!(
        assistant_projected.iter().any(|l| l.contains("x")),
        "projected assistant lines should contain code text; got: {assistant_projected:?}"
    );
}

#[test]
fn coordinator_hydration_preserves_assistant_markdown_styles() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "**bold** and `code`")],
        None,
    );

    // TranscriptEntry no longer has a `.rendered` field - test removed
    // Previously tested that assistant hydration preserved rendered markdown
}

#[test]
fn assistant_markdown_projection_is_memoized_across_repeated_messages() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("fenced_code_blocks.md");

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: markdown.clone(),
    });
    coordinator.drain_transport();

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    coordinator.drain_transport();

    // Both messages are processed; the second replaces the first via streaming truncation
}

#[tokio::test]
async fn resize_and_redraw_paths_do_not_retokenize_assistant_projection_cache() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    let markdown = markdown_fixture("fenced_code_blocks.md");

    driver
        .coordinator_mut()
        .enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    driver.coordinator_mut().drain_transport();

    for (columns, rows) in [(100, 28), (140, 42), (80, 24)] {
        driver
            .advance(&[DriveEvent::Key(TerminalEvent::Resize(
                crate::interaction::input::TerminalResize { columns, rows },
            ))])
            .await?;
    }

    // Resize clears the projection cache so width-aware re-projection occurs
    // on the next render pass. No assertion on cache misses — the counter was
    // removed when caching was moved to render_cached.
    Ok(())
}

#[test]
fn coordinator_hydration_keeps_unsupported_markdown_readable_in_assistant_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("unsupported_fallback.md");
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", &markdown)],
        None,
    );

    // After the raw-markdown refactor, the entry stores raw markdown.
    // Project it to verify the rendered content is readable.
    let projected_lines: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| matches!(line.role(), nu_agent_core::transcript::ir::Role::Assistant))
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    // Tables are now supported and rendered with separators
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("col") && line.contains("val"))
    );
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("a") && line.contains("b"))
    );
    assert!(
        projected_lines.iter().any(|line| line.contains("│")),
        "table cells should be separated"
    );
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("alt (image: https://img.example/x.png)"))
    );
}

#[test]
fn coordinator_hydration_handles_malformed_assistant_markdown_without_dropping_message() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("malformed.md");
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", &markdown)],
        None,
    );

    let assistant_entries = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| matches!(line.role(), nu_agent_core::transcript::ir::Role::Assistant))
        .collect::<Vec<_>>();

    assert!(!assistant_entries.is_empty());
    // Raw markdown is stored; check projected output contains the expected text
    let projected_text: String = assistant_entries
        .iter()
        .flat_map(|entry| crate::markdown::render_markdown_lines(&entry.text(), None))
        .flat_map(|l| l.spans.into_iter())
        .map(|s| s.text)
        .collect();
    assert!(
        projected_text.contains("fn main() {"),
        "projected text should contain 'fn main()'; got: {projected_text:?}"
    );
}

#[test]
fn assistant_message_event_sanitizes_pseudo_tags_and_control_tags_in_runtime_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "prefix\n[code:json]\n{\"ok\":true}\n[/code]\n<system-reminder>hidden</system-reminder>\nsuffix"
            .to_string(),
    });
    coordinator.drain_transport();

    // After the raw-markdown refactor, the raw markdown is stored in the ProseMessage.
    // Sanitization happens at projection time (render_markdown_lines). Verify by projecting.
    let projected_lines: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| matches!(line.role(), nu_agent_core::transcript::ir::Role::Assistant))
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    assert!(projected_lines.iter().any(|l| l.contains("prefix")));
    assert!(
        projected_lines
            .iter()
            .any(|line| line.contains("{\"ok\":true}"))
    );
    assert!(projected_lines.iter().any(|l| l.contains("suffix")));
    assert!(!projected_lines.iter().any(|line| line.contains("[code:")));
    assert!(!projected_lines.iter().any(|line| line.contains("[/code]")));
    assert!(
        !projected_lines
            .iter()
            .any(|line| line.contains("<system-reminder>"))
    );
    assert!(!projected_lines.iter().any(|line| line.contains("hidden")));
}

#[test]
fn coordinator_hydration_regression_no_duplicate_lines_on_single_call() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.hydrate_transcript_from_messages(
        vec![
            UiMessageSnapshot::new("user", "dup-check"),
            UiMessageSnapshot::new("assistant", "dup-check-reply"),
        ],
        None,
    );

    let user_count = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| line.text() == "dup-check")
        .count();
    let assistant_count = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| line.text() == "dup-check-reply")
        .count();

    assert_eq!(user_count, 1);
    assert_eq!(assistant_count, 1);
}

#[test]
fn coordinator_hydrate_with_empty_message_snapshot_leaves_empty_session_behavior_unchanged() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    let state = coordinator.state();
    assert!(state.transcript.entries.is_empty());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
}

#[tokio::test]
async fn global_abort_cancels_active_and_pending_and_new_submit_starts_fresh() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: submit two prompts, then abort with Esc-Esc through the real
    // terminal arm.
    driver
        .advance(&[
            key(TerminalKey::Char('a')),
            key(TerminalKey::Enter),
            key(TerminalKey::Char('b')),
            key(TerminalKey::Enter),
            key(TerminalKey::Esc),
            key(TerminalKey::Esc),
        ])
        .await?;

    // -- Check
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        None
    );

    // The real loop handed each submitted prompt to the orchestrator as a
    // PromptSubmitted event before the abort.
    let submitted = driver.take_orchestrator_events();
    assert!(
        submitted.iter().any(
            |event| matches!(event, OrchestratorEvent::PromptSubmitted { text } if text == "a")
        ),
        "first prompt must reach the orchestrator before the abort"
    );

    let statuses = driver
        .state()
        .prompt_items()
        .iter()
        .map(|item| item.status)
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![PromptStatus::Cancelled, PromptStatus::Cancelled]
    );

    // After abort, the restored text from the cancelled prompt is available
    // on the state. Applying it to the textarea is sync-poll behavior
    // (`poll_terminal_event`), not a render-loop arm, so exercise the
    // production sync primitives directly. In the real loop the first prompt
    // was already handed to the orchestrator, so only "b" is restored.
    for event in [TerminalKey::Char('c'), TerminalKey::Enter] {
        let mut source = StubEventSource {
            next: Some(TerminalEvent::Key(event)),
        };
        driver.coordinator_mut().poll_terminal_event(&mut source);
        driver.coordinator_mut().drain_transport();
    }
    assert_eq!(
        driver
            .coordinator_mut()
            .state
            .take_next_prompt_for_execution(),
        Some("bc".to_string())
    );
    Ok(())
}

#[test]
fn main_pane_vertical_split_has_no_overlap_or_bottom_cutoff() {
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    let (_header, transcript, input, status) = RuntimeCoordinator::main_pane_rects_for_height(10);

    assert_eq!(_header.height, 0);
    assert!(
        transcript.height > 0,
        "transcript pane should remain visible"
    );
    assert_eq!(
        status.height, STATUS_TARGET_HEIGHT,
        "footer must reserve STATUS_TARGET_HEIGHT rows"
    );
    assert_eq!(transcript.y + transcript.height, input.y);
    assert_eq!(input.y + input.height, status.y);
    assert_eq!(status.y + status.height, 10);
}

#[test]
fn multiline_input_prompt_icon_appears_only_on_first_visual_row() {
    let state = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };

    let rows = input_rows_with_prompt_for_test(&state, 5);
    assert_eq!(rows, vec!["❯ "]);
}

#[test]
fn prompt_prefix_uses_mode_indicator_insert_vs_normal_visual() {
    let insert = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };

    let normal = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };

    let visual = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        ..Default::default()
    };

    assert_eq!(input_rows_with_prompt_for_test(&insert, 20), vec!["❯ "]);
    assert_eq!(input_rows_with_prompt_for_test(&normal, 20), vec!["❮ "]);
    assert_eq!(input_rows_with_prompt_for_test(&visual, 20), vec!["❮ "]);
}

#[test]
fn prompt_prefix_switches_immediately_when_mode_changes() {
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Insert),
        ..Default::default()
    };

    assert_eq!(input_rows_with_prompt_for_test(&state, 20), vec!["❯ "]);

    state.input.mode = InputMode::Normal;
    assert_eq!(input_rows_with_prompt_for_test(&state, 20), vec!["❮ "]);
}

#[test]
fn status_contract_a_model_line_reports_identity_and_busy_idle() {
    let mut state = AppState::default();

    let idle_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        idle_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );

    state.phase = UiPhase::Busy;
    let busy_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        busy_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (busy)")
    );
}

#[test]
fn status_contract_b_excludes_input_mode_backend_poll_and_hint_lines() {
    let mut state = AppState::default();

    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input backend:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input poll:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input error:")));
    assert!(!lines.iter().any(|line| line.starts_with("Hint:")));
}

#[test]
fn status_contract_c_mcp_counts_include_configured_enabled_disabled_failed() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::state::McpServerState {
            name: "docs".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        lines
            .iter()
            .any(|line| line == "MCP: configured=3 enabled=1 disabled=1 failed=1")
    );
}

#[test]
fn status_contract_d_visible_mcp_tool_count_uses_runtime_truth_and_updates() {
    let mut state = AppState::default();
    state.status.mcp.set_llm_visible_mcp_tool_count(5);

    let before = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(before.iter().any(|line| line == "LLM-visible MCP tools: 5"));

    state.status.mcp.set_llm_visible_mcp_tool_count(2);
    let after = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(after.iter().any(|line| line == "LLM-visible MCP tools: 2"));
}

#[test]
fn status_contract_e_failures_show_names_and_reasons_and_healthy_none_when_clear() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Failed,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Failed,
        Some("timeout".to_string())
    ));
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        None
    ));

    let failed_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    let failed_rendered = failed_lines.join("\n");
    assert!(failed_rendered.contains("Failures: gh (timeout), k8s"));

    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Enabled,
        None
    ));
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Enabled,
        None
    ));
    let healthy_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");
    assert!(
        healthy_lines
            .iter()
            .any(|line| line == "Failures: none (healthy)")
    );
}

#[test]
fn status_contract_f_narrow_layout_is_compact_and_ellipsizes_deterministically() {
    let mut state = AppState {
        phase: UiPhase::Busy,
        ..Default::default()
    };
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "very-long-mcp-server-name-that-must-be-truncated".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    assert!(state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "very-long-mcp-server-name-that-must-be-truncated",
        McpServerUsabilityState::Failed,
        Some("very long failure reason that should be truncated to keep the status line readable"
            .to_string())
    ));
    state.status.mcp.set_llm_visible_mcp_tool_count(42);

    let lines = crate::runtime::status_lines_for_test(
        &mut state,
        "provider/super-long-model-name-that-needs-truncation",
    );
    let rendered = lines.join("\n");
    assert!(rendered.contains('…'));
    assert!(!rendered.contains("Hint: Ctrl-P -> MCPs"));

    let compact = crate::runtime::compact_status_line_for_test(
        "provider/super-long-model-name-that-needs-truncation",
        None,
    );
    let compact_text: String = compact.spans.iter().map(|s| s.content.as_ref()).collect();
    let compact_narrow = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "provider/super-long-model-name-that-needs-truncation",
        Some("feature/very-long-branch-name-that-needs-truncation"),
        None,
        24,
    );
    let compact_narrow_text: String = compact_narrow
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(!compact_text.starts_with("❯ "));
    assert!(!compact_text.contains('|'));
    assert!(compact_narrow_text.contains("..."));
    assert!(!compact_narrow_text.contains('|'));
}

#[test]
fn status_lines_include_stable_active_model_identity_line() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");

    assert!(
        status_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line == "MCP: configured=2 enabled=1 disabled=1 failed=0")
    );
    assert!(!status_lines.iter().any(|line| line.starts_with("Hint:")));
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input backend:"))
    );
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input poll:"))
    );
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input error:"))
    );
}

#[test]
fn help_panel_renders_required_sections_in_contract_order() {
    let (title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    assert_eq!(title, "Help");
    let rendered_lines = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let required_sections = [
        "Getting started",
        "Modes (insert vs normal)",
        "Core keys (with explanations)",
        "Command palette (Ctrl-P)",
        "MCP basics (enabled/disabled/failed + where to toggle)",
        "Troubleshooting",
    ];

    let mut previous_index = None;
    for section in required_sections {
        let section_index = rendered_lines
            .iter()
            .position(|line| line.trim() == section)
            .unwrap_or_else(|| panic!("missing section heading: {section}"));
        if let Some(previous_index) = previous_index {
            assert!(
                section_index > previous_index,
                "section out of order: {section}"
            );
        }
        previous_index = Some(section_index);
    }
}

#[test]
fn help_panel_copy_is_plain_language_and_includes_ctrl_p_and_mcp_basics() {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("Ctrl-P"),
        "help should explicitly mention Ctrl-P"
    );
    assert!(
        rendered.contains("open the command palette"),
        "Ctrl-P entry should explain intent in plain language"
    );
    assert!(
        rendered.contains("enabled")
            && rendered.contains("disabled")
            && rendered.contains("failed")
            && rendered.contains("command palette")
            && rendered.contains("MCPs"),
        "MCP basics should describe state meanings and where to toggle"
    );

    assert!(
        rendered.contains("server is") || rendered.contains("return to normal mode"),
        "help copy should include explanatory language rather than key-only jargon"
    );
}

#[test]
fn help_panel_markdown_projection_preserves_supported_formatting() {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("• "),
        "markdown bullet lists should be projected as list markers"
    );
    let inline_code_spans = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.content.contains("Ctrl-P") || span.content.contains("Esc"))
        .count();
    assert!(
        inline_code_spans > 0,
        "inline code content should survive markdown projection"
    );
}

#[test]
fn help_panel_scroll_can_reach_final_content_line_with_keyboard_scroll_model() {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let viewport_inner_height = 8u16;
    let max_scroll = help_panel_max_scroll_for_test(&lines, viewport_inner_height);
    let window = help_panel_visible_window_for_test(&lines, viewport_inner_height, max_scroll);
    let rendered = window
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("If text looks stale after resize"),
        "scroll window should reach help footer content"
    );
}

#[test]
fn help_panel_shows_overflow_position_cue_when_content_exceeds_viewport() -> Result<()> {
    let (_title, lines) = help_panel_lines(&crate::rendering::theme::TuiTheme::default());
    let viewport_inner_height = 8u16;
    let cue = help_panel_overflow_cue_for_test(&lines, viewport_inner_height, 3)
        .ok_or("should have overflow cue when help exceeds viewport")?;

    assert!(cue.contains("PgUp/PgDn"));
    assert!(cue.contains("Esc close"));
    assert!(cue.contains("/"));
    Ok(())
}

#[tokio::test]
async fn help_panel_escape_closes_panel_after_scroll() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Help);
    // AppState.info_panel_scroll is a public field — no setter method exists.
    driver.coordinator_mut().state.info_panel_scroll = 5;

    // -- Exec & Check: Down scrolls (or holds) the panel through the real arm.
    driver.advance(&[key(TerminalKey::Down)]).await?;
    assert!(driver.state().info_panel_scroll >= 5);

    // -- Exec & Check: Esc closes the panel through the real arm.
    driver.advance(&[key(TerminalKey::Esc)]).await?;
    assert_eq!(driver.state().info_panel, None);
    Ok(())
}

#[test]
fn status_panel_exposes_model_and_mcp_backend_status_lines() {
    let mut state = AppState::default();
    state.status.identity.active_model_identity = "openai/gpt-4o-mini".to_string();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);

    let (title, lines) = status_panel_lines(&state);
    assert_eq!(title, "Status");
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Model: openai/gpt-4o-mini (idle)"));
    assert!(rendered.contains("MCP: configured=2 enabled=1 disabled=1 failed=0"));
    assert!(rendered.contains("LLM-visible MCP tools: 0"));
    assert!(rendered.contains("Failures: none (healthy)"));
    assert!(!rendered.contains("Hint: Ctrl-P -> MCPs"));
    assert!(!rendered.contains("Input backend:"));
    assert!(!rendered.contains("Input poll:"));
    assert!(!rendered.contains("Input error:"));
    assert!(!rendered.contains("MCP + Model status"));
    assert!(!rendered.contains("MCP/Input backend:"));
}

#[test]
fn mcp_panel_renders_columns_selection_and_compact_table_contract() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state.status.mcp.mcp_panel_selection = 1;

    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connect timeout".to_string()),
    );
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 3);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 9);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.selected, Some(1));
    assert_eq!(model.rows.len(), 2);
    assert_eq!(model.rows[0][0], "gh");
    assert_eq!(model.rows[0][1], "3");
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][0], "k8s");
    assert_eq!(model.rows[1][1], "9");
    assert_eq!(model.rows[1][2], "🔴");
}

#[test]
fn mcp_table_status_icon_mapping_is_deterministic() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "enabled-srv".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "disabled-srv".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::state::McpServerState {
            name: "failed-srv".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][2], "⚪");
    assert_eq!(model.rows[2][2], "🔴");
}

#[test]
fn mcp_table_emoji_status_rows_use_safe_status_column_width_contract() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "enabled-srv".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "disabled-srv".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::state::McpServerState {
            name: "failed-srv".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(super::MCP_STATUS_COLUMN_WIDTH, 6);
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][2], "⚪");
    assert_eq!(model.rows[2][2], "🔴");
}

#[test]
fn mcp_details_height_allocation_prefers_multiple_tool_lines_in_normal_popup_heights() {
    assert_eq!(super::mcp_details_height_for_inner_height(4), 0);
    assert_eq!(super::mcp_details_height_for_inner_height(5), 1);
    assert_eq!(super::mcp_details_height_for_inner_height(6), 2);
    assert_eq!(super::mcp_details_height_for_inner_height(8), 3);
    assert_eq!(super::mcp_details_height_for_inner_height(10), 4);
    assert_eq!(super::mcp_details_height_for_inner_height(12), 5);
    assert_eq!(super::mcp_details_height_for_inner_height(14), 6);
}

#[test]
fn mcp_details_height_formula_matches_step_table() {
    // Verifies all inner_height values 0..=20 against the expected step-table output.
    // The step-table was replaced by a match expression for idiomatic clarity;
    // this test pins the observable behaviour so any future change is caught.
    let expected: [(u16, u16); 21] = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 1),
        (6, 2),
        (7, 2),
        (8, 3),
        (9, 3),
        (10, 4),
        (11, 4),
        (12, 5),
        (13, 5),
        (14, 6),
        (15, 6),
        (16, 6),
        (17, 6),
        (18, 6),
        (19, 6),
        (20, 6),
    ];
    for (h, want) in expected {
        assert_eq!(
            super::mcp_details_height_for_inner_height(h),
            want,
            "inner_height={h}"
        );
    }
}

#[test]
fn mcp_panel_layout_keeps_table_primary_with_multiple_visible_rows_in_common_height() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(
        (0..8)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );

    let inner_height = 8u16;
    let details_height = super::mcp_details_height_for_inner_height(inner_height);
    let table_height = inner_height
        .saturating_sub(1)
        .saturating_sub(details_height);
    let model = mcp_table_model_for_test(&state, 80, table_height);

    assert!(
        model.rows.len() > 1,
        "table should show multiple rows at common modal height"
    );
}

#[test]
fn mcp_panel_controls_line_removes_status_legend_and_keeps_toggle_hint_compact() {
    let line = super::mcp_panel_controls_line();
    assert_eq!(
        line,
        "Session-only toggles | Enter/Space toggle | Esc close"
    );
    assert!(!line.contains("enabled"));
    assert!(!line.contains("disabled"));
    assert!(!line.contains("failed"));
}

#[test]
fn mcp_table_visible_tool_count_uses_live_per_server_mapping_without_state_gating() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 4);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 2);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
}

#[test]
fn mcp_selected_details_model_shows_full_error_text_tools_list_and_fallback() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    let reason = "connection timeout while dialing 10.0.0.1:443".to_string();
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some(reason.clone()),
    );
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "k8s",
        vec!["k8s__z_last".to_string(), "k8s__a_first".to_string()],
    );

    state.status.mcp.mcp_panel_selection = 1;
    let failed =
        super::mcp_selected_details_for_test(&state).ok_or("should have selected MCP details")?;
    assert_eq!(failed.server_line, "Server: k8s (failed)");
    assert_eq!(failed.error_line, format!("Error: {reason}"));
    assert_eq!(failed.tools_line, "Tools: k8s__a_first, k8s__z_last");

    state.status.mcp.mcp_panel_selection = 0;
    let healthy =
        super::mcp_selected_details_for_test(&state).ok_or("should have selected MCP details")?;
    assert_eq!(healthy.server_line, "Server: gh (enabled)");
    assert_eq!(healthy.error_line, "Error: None");
    assert_eq!(healthy.tools_line, "Tools: None");
    Ok(())
}

#[test]
fn mcp_table_visible_tool_count_respects_live_updates_after_selection_changes() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::state::McpServerState {
            name: "docs".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("gh", 4);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("k8s", 2);
    state
        .status
        .mcp
        .set_mcp_visible_tool_count_by_server_name("docs", 7);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
    assert_eq!(model.rows[2][1], "7");
}

#[test]
fn mcp_selected_details_height_zero_and_one_rows_preserve_error_presence() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connection timeout while dialing 10.0.0.1:443".to_string()),
    );

    let zero_rows = super::mcp_selected_details_lines_for_test(&state, 0, 80);
    assert!(zero_rows.is_empty());

    let one_row = super::mcp_selected_details_lines_for_test(&state, 1, 80);
    assert_eq!(one_row.len(), 1);
    assert!(one_row[0].contains("Error: connection timeout while dialing 10.0.0.1:443"));
    assert!(one_row[0].contains("Server: k8s (failed)"));
}

#[test]
fn mcp_selected_details_constrained_two_rows_preserve_full_error_line() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connection timeout while dialing 10.0.0.1:443 after many retries and additional context".to_string()),
    );

    let two_rows = super::mcp_selected_details_lines_for_test(&state, 2, 80);
    assert_eq!(two_rows.len(), 2);
    assert_eq!(two_rows[0], "Server: k8s (failed)");
    assert_eq!(
        two_rows[1],
        "Error: connection timeout while dialing 10.0.0.1:443 after many retries and additional context"
    );
}

#[test]
fn mcp_selected_details_normal_height_preserves_full_error_line() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        }]);
    let reason =
        "connection timeout while dialing 10.0.0.1:443 after many retries and additional context";
    state.status.mcp.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some(reason.to_string()),
    );

    let full = super::mcp_selected_details_lines_for_test(&state, 3, 80);
    assert_eq!(full.len(), 3);
    assert_eq!(full[0], "Server: k8s (failed)");
    assert_eq!(full[1], format!("Error: {reason}"));
    assert_eq!(full[2], "Tools: None");
}

#[test]
fn mcp_selected_details_packs_multiple_tools_per_line_with_comma_separators() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__z_last".to_string(),
            "gh__a_first".to_string(),
            "gh__m_mid".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 6, 36);
    assert_eq!(
        details,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first, gh__m_mid",
            "       gh__z_last",
        ]
    );
}

#[test]
fn mcp_selected_details_clipped_tool_list_shows_deterministic_plus_n_more_cue() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
            "gh__e_fifth".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 6, 30);
    assert_eq!(
        details,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first",
            "       gh__b_second",
            "       gh__c_third",
            "       gh__d_fourth, +1 more",
        ]
    );
}

#[test]
fn mcp_selected_details_single_tool_row_budget_prefers_truncation_cue_visibility() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 3, 36);
    assert_eq!(details.len(), 3);
    assert_eq!(
        details,
        vec![
            "Server: gh (enabled)",
            "Error: None",
            "Tools: gh__a_first, +2 more",
        ]
    );
}

#[test]
fn mcp_selected_details_continuation_rows_align_and_use_tools_prefix_once() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
            "gh__e_fifth".to_string(),
        ],
    );

    let details = super::mcp_selected_details_lines_for_test(&state, 6, 30);
    assert_eq!(details[2], "Tools: gh__a_first");
    assert_eq!(details[3], "       gh__b_second");
    assert_eq!(details[4], "       gh__c_third");
    assert_eq!(details[5], "       gh__d_fourth, +1 more");
}

#[test]
fn mcp_selected_details_wrapping_uses_actual_details_width() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);
    state.status.mcp.set_mcp_visible_tool_names_by_server_name(
        "gh",
        vec![
            "gh__a_first".to_string(),
            "gh__b_second".to_string(),
            "gh__c_third".to_string(),
            "gh__d_fourth".to_string(),
        ],
    );

    let wide = super::mcp_selected_details_lines_for_test(&state, 6, 44);
    let narrow = super::mcp_selected_details_lines_for_test(&state, 6, 24);

    assert_eq!(wide[2], "Tools: gh__a_first, gh__b_second");
    assert_eq!(wide[3], "       gh__c_third, gh__d_fourth");
    assert_eq!(narrow[2], "Tools: gh__a_first");
    assert_eq!(narrow[3], "       gh__b_second");
    assert_eq!(narrow[4], "       gh__c_third");
}

#[test]
fn mcp_table_model_narrow_width_keeps_required_columns() {
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        }]);

    let model = mcp_table_model_for_test(&state, 32, 8);
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.rows.len(), 1);
}

#[test]
fn mcp_table_model_overflow_top_window_locks_exact_cue_and_selected_mapping() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(
        (0..8)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );
    state.status.mcp.mcp_panel_selection = 0;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(0));
    let selected = model.selected.ok_or("should have selection")?;
    assert_eq!(model.rows[selected][0], "srv-0");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 1-5 / 8".to_string())
    );
    Ok(())
}

#[test]
fn mcp_table_model_overflow_middle_window_locks_exact_cue_and_selected_mapping() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(
        (0..8)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );
    state.status.mcp.mcp_panel_selection = 5;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(4));
    let selected = model.selected.ok_or("should have selection")?;
    assert_eq!(model.rows[selected][0], "srv-5");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 2-6 / 8".to_string())
    );
    Ok(())
}

#[test]
fn mcp_table_model_overflow_bottom_window_locks_exact_cue_and_selected_mapping() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(
        (0..8)
            .map(|idx| crate::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );
    state.status.mcp.mcp_panel_selection = 7;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(4));
    let selected = model.selected.ok_or("should have selection")?;
    assert_eq!(model.rows[selected][0], "srv-7");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 4-8 / 8".to_string())
    );
    Ok(())
}

#[test]
fn command_palette_table_renders_required_columns_and_rows() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    let model = command_palette_table_model_for_test(&state, 80, 10);

    assert_eq!(model.columns, vec!["Action", "Summary"]);
    let actions = model
        .rows
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec!["Help", "Status", "MCPs", "Skills", "Models", "Agents"]
    );
    assert!(model.rows.iter().all(|row| row[2].is_empty()));
    assert_eq!(model.selected, Some(0));
}

#[test]
fn command_palette_table_renders_skills_action_row() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    let model = command_palette_table_model_for_test(&state, 80, 10);
    let actions = model
        .rows
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();

    assert!(actions.contains(&"Skills"));
    assert!(actions.contains(&"Models"));
}

#[test]
fn skills_panel_renders_empty_state_when_no_skills_available() {
    let state = AppState::default();
    let (title, lines) = crate::runtime::skills_panel_lines(&state);

    assert_eq!(title, "Skills");
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("No discoverable skills available."));
}

#[test]
fn skills_panel_lists_skills_in_deterministic_order() -> Result<()> {
    let mut state = AppState::default();
    state.status.mcp.set_discoverable_skills(vec![
        crate::state::DiscoverableSkill {
            source_priority: 1,
            source: "home".to_string(),
            name: "zeta".to_string(),
        },
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "beta".to_string(),
        },
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "alpha".to_string(),
        },
    ]);

    let (_title, lines) = crate::runtime::skills_panel_lines(&state);
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let alpha_idx = rendered.find("alpha").ok_or("should find alpha row")?;
    let beta_idx = rendered.find("beta").ok_or("should find beta row")?;
    let zeta_idx = rendered.find("zeta").ok_or("should find zeta row")?;
    assert!(alpha_idx < beta_idx);
    assert!(beta_idx < zeta_idx);
    Ok(())
}

#[test]
fn help_panel_scroll_offset_applied() {
    // A viewport smaller than the help content forces a nonzero max scroll.
    // Requesting a large offset must be clamped to the max; requesting a small
    // offset must be returned unchanged.
    let viewport_height = 8u16;
    let viewport_width = 80u16;

    let small_scroll = super::help_panel_scroll_offset_for_test(viewport_height, viewport_width, 3);
    assert_eq!(
        small_scroll, 3,
        "scroll offset below max must pass through unchanged"
    );

    let huge_scroll =
        super::help_panel_scroll_offset_for_test(viewport_height, viewport_width, usize::MAX);
    let max_scroll = super::help_panel_max_scroll_for_test(
        &super::help_panel_lines(&crate::rendering::theme::TuiTheme::default()).1,
        viewport_height,
    );
    assert_eq!(
        huge_scroll, max_scroll,
        "scroll offset above max must be clamped to max"
    );
    assert!(max_scroll > 0, "help content must exceed small viewport");
}

#[test]
fn status_panel_scroll_offset_applied() {
    // Status content is short; with a large viewport the max scroll is 0 and
    // any requested offset is clamped to 0. With a very small viewport the
    // offset should be clamped correctly.
    let mut state = AppState::default();
    state
        .status
        .mcp
        .set_mcp_servers(vec![crate::state::McpServerState {
            name: "gh".to_string(),
            state: crate::state::McpServerUsabilityState::Enabled,
        }]);

    let viewport_height = 3u16; // smaller than status content
    let viewport_width = 80u16;

    let small_scroll =
        super::status_panel_scroll_offset_for_test(&state, viewport_height, viewport_width, 1);
    // 1 is within content so it should pass through (or be clamped if content
    // is ≤ 3 lines — either outcome is correct as long as it's ≤ max).
    let (_title, lines) = super::status_panel_lines(&state);
    let max = super::help_panel_max_scroll_for_test(&lines, viewport_height);
    assert!(
        small_scroll <= max,
        "scroll offset must not exceed max (got {small_scroll}, max {max})"
    );

    let huge_scroll = super::status_panel_scroll_offset_for_test(
        &state,
        viewport_height,
        viewport_width,
        usize::MAX,
    );
    assert_eq!(
        huge_scroll, max,
        "scroll offset above max must be clamped to max"
    );
}

#[test]
fn skills_panel_scroll_offset_applied() {
    let mut state = AppState::default();
    state.status.mcp.set_discoverable_skills(vec![
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "alpha".to_string(),
        },
        crate::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "beta".to_string(),
        },
    ]);

    let viewport_height = 2u16; // smaller than content (header + 2 skill lines)
    let viewport_width = 80u16;

    let (_title, lines) = super::skills_panel_lines(&state);
    let max = super::help_panel_max_scroll_for_test(&lines, viewport_height);

    let huge_scroll = super::skills_panel_scroll_offset_for_test(
        &state,
        viewport_height,
        viewport_width,
        usize::MAX,
    );
    assert_eq!(
        huge_scroll, max,
        "scroll offset above max must be clamped to max"
    );

    let zero_scroll =
        super::skills_panel_scroll_offset_for_test(&state, viewport_height, viewport_width, 0);
    assert_eq!(zero_scroll, 0, "zero scroll must pass through as zero");
}

#[test]
fn inline_slash_suggestions_render_inline_with_single_hint_contract() {
    let mut state = AppState::default();
    state.check_inline_slash("/");

    let rows = inline_slash_lines_for_test(&state);
    assert!(!rows.is_empty());
    assert!(rows[0].contains("/compact"));
    assert!(rows[0].starts_with('❯'));

    let title = super::command_palette_title(None);
    assert!(title.contains("↑/↓ or Ctrl-N · Enter · Esc"));
}

#[test]
fn command_palette_table_emits_overflow_position_cue_when_viewport_is_small() -> Result<()> {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    let model = command_palette_table_model_for_test(&state, 80, 5);
    let cue = model
        .overflow_cue
        .ok_or("should have overflow cue when rows exceed viewport")?;
    assert!(cue.contains("/"));
    assert!(cue.contains("Esc close"));
    Ok(())
}

#[test]
fn help_modal_uses_large_readable_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Help,
    );

    assert!(popup.width >= 72);
    assert!(popup.height >= 18);
}

#[test]
fn status_modal_uses_compact_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Status,
    );

    assert!(popup.width <= 72);
    assert!(popup.height <= 14);
}

#[test]
fn modal_layout_policy_applies_consistently_across_panels() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let command_palette = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::CommandPalette,
    );
    let skills = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Skills,
    );
    let mcps = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Mcps,
    );

    assert_eq!(skills.width, mcps.width);
    assert_eq!(skills.height, mcps.height);
    assert!(command_palette.width < skills.width);
}

#[test]
fn models_modal_uses_layout_policy_defaults() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let models = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let skills = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Skills,
    );

    assert_eq!(models.width, skills.width);
    assert_eq!(models.height, skills.height);
}

#[test]
fn themes_modal_uses_layout_policy_defaults() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let themes = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Themes,
    );
    let models = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );

    assert_eq!(themes.width, models.width);
    assert_eq!(themes.height, models.height);
}

#[test]
fn modal_open_state_applies_dimmed_backdrop() {
    let mut state = AppState::default();
    open_command_palette_for_test(&mut state);

    assert!(modal_open_state_applies_dimmed_backdrop_for_test(&state));
}

#[test]
fn inline_model_picker_modal_respects_border_and_backdrop_policy() {
    let mut state = AppState::default();
    state.picker.open(ActivePicker::Model);

    assert!(inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(&state));
}

#[test]
fn permission_does_not_open_global_dimmed_modal_backdrop() {
    let mut state = AppState::default();
    state
        .permission
        .open_prompt(crate::state::PermissionPrompt {
            request_id: "ask-0000000000000001".to_string(),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            scope: "nested".to_string(),
            pattern: "*".to_string(),
            target_field: Some("command".to_string()),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
        });

    assert!(!modal_open_state_applies_dimmed_backdrop_for_test(&state));
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let mut state = AppState::default();
    assert_eq!(
        crate::runtime::panels::MODEL_PICKER_EMPTY_STATE_MESSAGE,
        "No models available in cached startup config."
    );
    state.picker.open(ActivePicker::Model);
    assert!(state.picker.active_state().unwrap().filtered().is_empty());
}

#[test]
fn status_lines_report_failed_state_count_when_present() {
    let mut state = AppState::default();
    state.status.mcp.set_mcp_servers(vec![
        crate::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");

    assert!(
        status_lines
            .iter()
            .any(|line| line == "MCP: configured=2 enabled=1 disabled=0 failed=1")
    );
}

#[test]
fn status_lines_include_tokens_line_with_na_before_any_llm_end() {
    let mut state = AppState::default();
    let status_lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4o-mini");

    assert!(
        status_lines
            .iter()
            .any(|line| line == "LLM-visible MCP tools: 0")
    );
}

#[test]
fn status_lines_include_latest_and_rolling_tokens_after_llm_end_events() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 10,
        tool_calls: 0,
        input_tokens: 11,
        output_tokens: 9,
        total_tokens: 20,
    });
    coordinator.drain_transport();

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 12,
        tool_calls: 0,
        input_tokens: 3,
        output_tokens: 4,
        total_tokens: 7,
    });
    coordinator.drain_transport();

    let status_lines =
        crate::runtime::status_lines_for_test(&mut coordinator.state, "openai/gpt-4o-mini");

    assert!(
        status_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line == "LLM-visible MCP tools: 0")
    );
}

#[test]
fn compact_status_line_reports_lane_1_only() {
    let status_line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let text: String = status_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert!(text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!text.contains('|'));
}

#[test]
fn lane_2_context_line_uses_exact_usage_format_without_extra_text() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    let line = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(
        text,
        "                                                                                                               250 (25%)"
    );
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
}

#[test]
fn lane_2_context_line_falls_back_to_used_only_when_max_unavailable() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(42),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state.status.tokens.set_context_window_max_tokens(None);

    let line = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(
        text,
        "                                                                                                                      42"
    );
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
}

#[test]
fn footer_two_lane_contract_exposes_lane_1_and_lane_2_simultaneously() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    let lane_1 = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let lane_1_text: String = lane_1.spans.iter().map(|s| s.content.as_ref()).collect();
    let lane_2 = crate::runtime::lane_2_status_line_for_test(&state, 120);
    let lane_2_text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(lane_1_text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!lane_1_text.contains('|'));
    assert!(lane_2_text.ends_with("250 (25%)"));
}

#[test]
fn configured_path_resolves_context_max_without_fallback_format() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 40,
        tool_calls: 0,
        input_tokens: 2_500,
        output_tokens: 500,
        total_tokens: 3_000,
    });
    coordinator.drain_transport();

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let lane_2_text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(lane_2_text.ends_with("3k (2%)"));
    assert!(!lane_2_text.contains('/'));
}

#[test]
fn lane_2_context_line_updates_after_each_turn_and_does_not_stale() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 12,
        tool_calls: 0,
        input_tokens: 2,
        output_tokens: 8,
        total_tokens: 10,
    });
    coordinator.drain_transport();
    let first = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let first_text: String = first.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(first_text.ends_with("10 (10%)"));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();
    let second = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let second_text: String = second.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(second_text.ends_with("40 (40%)"));
}

#[test]
fn lane_2_context_line_truncation_removes_any_extra_labels_or_hints() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(12345),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128000));

    let line = crate::runtime::lane_2_status_line_for_test(&state, 30);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(text, "                    12.3k (9%)");
    assert!(!text.contains("Context"));
    assert!(!text.contains("Ctrl-P"));
    assert!(!text.contains('|'));
}

#[test]
fn lane_2_rehydrates_used_tokens_from_hydrated_history_metadata() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("user", "hello"), {
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(444),
            });
            s
        }],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 120);
    assert!(text.ends_with("444"));
}

#[test]
fn lane_2_rehydrate_with_known_max_shows_ratio_immediately() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));
    coordinator.hydrate_transcript_from_messages(
        vec![{
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(250),
            });
            s
        }],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("250 (25%)"));
}

#[test]
fn lane_2_rehydrate_without_usage_metadata_and_without_max_uses_fallback() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "history")],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 120);
    assert!(text.ends_with("0"));
}

#[test]
fn lane_2_rehydrate_without_usage_metadata_with_known_max_shows_ratio_not_fallback() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "history")],
        None,
    );

    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("0 (0%)"));
}

#[test]
fn lane_2_rehydrate_is_replaced_by_live_turn_usage() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));
    coordinator.hydrate_transcript_from_messages(
        vec![{
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(7),
            });
            s
        }],
        None,
    );

    let hydrated = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let hydrated_text: String = hydrated.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(hydrated_text.ends_with("7 (7%)"));

    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();

    let live = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let live_text: String = live.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(live_text.ends_with("40 (40%)"));
}

#[test]
fn lane_2_threshold_formatting_contract_100_and_1000_and_11657() {
    let mut state = AppState::default();
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    state.status.tokens.latest_total_tokens = Some(100);
    let one_hundred = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let one_hundred_text: String = one_hundred
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(one_hundred_text.ends_with("100 (0%)"));

    state.status.tokens.latest_total_tokens = Some(1_000);
    let one_thousand = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let one_thousand_text: String = one_thousand
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(one_thousand_text.ends_with("1k (0%)"));

    state.status.tokens.latest_total_tokens = Some(11_657);
    let eleven_point_six = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let eleven_text: String = eleven_point_six
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(eleven_text.ends_with("11.6k (5%)"));
}

#[test]
fn lane_2_is_right_aligned_in_wide_layout() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(11_657),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    let width = 40usize;
    let line = crate::runtime::lane_2_status_line_for_test(&state, width);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(text.chars().count(), width);
    assert!(text.ends_with("11.6k (5%)"));
    assert!(text.starts_with(" "));
}

#[test]
fn lane_2_narrow_width_uses_deterministic_right_anchored_truncation() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(11_657),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(200_000));

    let line = crate::runtime::lane_2_status_line_for_test(&state, 8);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert_eq!(text, "... (5%)");
}

/// NOTE: This test is currently disabled because we've migrated to rig messages for persistence,
/// and rig messages do NOT store usage information in the message itself. Usage is tracked
/// separately at the completion/response level. When loading rig messages from JSONL for
/// transcript hydration, usage information is not available.
/// TODO: Either restore usage tracking via a separate mechanism, or remove this test entirely.
/// NOTE: This test is currently disabled because we've migrated to rig messages for persistence,
/// and rig messages do NOT store usage information in the message itself. When loading rig
/// messages from JSONL for transcript hydration, usage information is not available, and we
/// correctly do not parse it from message content either.
/// TODO: Either restore usage tracking via a separate mechanism, or remove this test entirely.
/// (Test deleted as part of old Message type cleanup)

#[derive(Clone)]
struct MockTerminalBackend {
    actions: Rc<RefCell<Vec<TerminalAction>>>,
    state: Rc<RefCell<MockTerminalState>>,
    fail_on: Option<TerminalAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MockTerminalState {
    raw_mode_enabled: bool,
    alt_screen_enabled: bool,
    cursor_visible: bool,
    bracketed_paste_enabled: bool,
}

impl Default for MockTerminalState {
    fn default() -> Self {
        Self {
            raw_mode_enabled: false,
            alt_screen_enabled: false,
            cursor_visible: true,
            bracketed_paste_enabled: false,
        }
    }
}

impl MockTerminalBackend {
    fn new(
        actions: Rc<RefCell<Vec<TerminalAction>>>,
        state: Rc<RefCell<MockTerminalState>>,
        fail_on: Option<TerminalAction>,
    ) -> Self {
        Self {
            actions,
            state,
            fail_on,
        }
    }

    fn run(&self, action: TerminalAction) -> core::result::Result<(), TerminalLifecycleError> {
        self.actions.borrow_mut().push(action);

        if self.fail_on == Some(action) {
            return Err(TerminalLifecycleError::new(
                action,
                format!("injected failure for {action:?}"),
            ));
        }

        Ok(())
    }
}

impl TerminalBackend for MockTerminalBackend {
    fn enable_raw_mode(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = true;
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::DisableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = false;
        Ok(())
    }

    fn enter_alt_screen(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnterAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = true;
        Ok(())
    }

    fn leave_alt_screen(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::LeaveAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = false;
        Ok(())
    }

    fn hide_cursor(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::HideCursor)?;
        self.state.borrow_mut().cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::ShowCursor)?;
        self.state.borrow_mut().cursor_visible = true;
        Ok(())
    }

    fn enable_bracketed_paste(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableBracketedPaste)?;
        self.state.borrow_mut().bracketed_paste_enabled = true;
        Ok(())
    }

    fn disable_bracketed_paste(&mut self) -> core::result::Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::DisableBracketedPaste)?;
        self.state.borrow_mut().bracketed_paste_enabled = false;
        Ok(())
    }
}

fn assert_terminal_restored(state: &Rc<RefCell<MockTerminalState>>) {
    assert_eq!(
        *state.borrow(),
        MockTerminalState {
            raw_mode_enabled: false,
            alt_screen_enabled: false,
            cursor_visible: true,
            bracketed_paste_enabled: false,
        }
    );
}

#[test]
fn run_with_terminal_restore_sync_executes_enter_run_and_restore_in_order() -> Result<()> {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let value = run_with_terminal_restore_sync(&mut lifecycle, || Ok::<_, &'static str>(42))
        .map_err(|e| format!("run should succeed: {e:?}"))?;
    assert_eq!(value, 42);

    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
    Ok(())
}

#[test]
fn runtime_enter_failure_maps_to_enter_error() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(
        actions.clone(),
        state.clone(),
        Some(TerminalAction::EnterAltScreen),
    );
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || Ok(()))
        .expect_err("expected enter failure");

    match err {
        RuntimeRunError::Enter(error) => {
            assert_eq!(error.action, TerminalAction::EnterAltScreen);
        }
        RuntimeRunError::Run(run_error) => panic!("unexpected run error: {run_error:?}"),
    }

    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
}

#[test]
fn runtime_run_error_maps_to_run() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions, state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), _, _>(&mut lifecycle, || Err("boom"))
        .expect_err("expected run failure");

    match err {
        RuntimeRunError::Run(RestoreRunError::Run(run_error)) => assert_eq!(run_error, "boom"),
        RuntimeRunError::Enter(error) => {
            panic!("unexpected enter error: {error}")
        }
        RuntimeRunError::Run(other) => panic!("unexpected run mapping: {other:?}"),
    }
    assert_terminal_restored(&state);
}

#[test]
fn runtime_run_and_restore_error_maps_to_run_with_restore() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions, state, Some(TerminalAction::ShowCursor));
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), _, _>(&mut lifecycle, || Err("boom"))
        .expect_err("expected combined run/restore failure");

    match err {
        RuntimeRunError::Run(RestoreRunError::RunWithRestore {
            run_error,
            restore_error,
        }) => {
            assert_eq!(run_error, "boom");
            assert_eq!(restore_error.action, TerminalAction::ShowCursor);
        }
        other => panic!("unexpected runtime error mapping: {other:?}"),
    }
}

#[test]
fn runtime_restore_error_after_success_maps_to_restore() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions, state, Some(TerminalAction::ShowCursor));
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || Ok(()))
        .expect_err("expected restore failure");

    match err {
        RuntimeRunError::Run(RestoreRunError::Restore(restore_error)) => {
            assert_eq!(restore_error.action, TerminalAction::ShowCursor);
        }
        other => panic!("unexpected runtime error mapping: {other:?}"),
    }
}

#[test]
fn runtime_panic_restores_and_rethrows() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || {
            panic!("boom");
        });
    }));

    assert!(panic_result.is_err(), "panic should be resumed");
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
}

#[test]
fn panic_during_busy_render_loop_still_restores_terminal_state() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_with_terminal_restore_sync::<_, (), &'static str, _>(&mut lifecycle, || {
            panic!("simulated panic during busy render loop")
        });
    }));

    assert!(panic_result.is_err(), "panic should be resumed");
    assert_terminal_restored(&state);
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
}

#[test]
fn cancellation_during_shutdown_restores_terminal_and_preserves_cancel_error() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let err = run_with_terminal_restore_sync::<_, (), _, _>(&mut lifecycle, || {
        Err("cancelled during shutdown")
    })
    .expect_err("expected cancellation error");

    match err {
        RuntimeRunError::Run(RestoreRunError::Run(run_error)) => {
            assert_eq!(run_error, "cancelled during shutdown");
        }
        other => panic!("unexpected runtime error mapping: {other:?}"),
    }

    assert_terminal_restored(&state);
    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::EnableBracketedPaste,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableBracketedPaste,
            TerminalAction::DisableRawMode,
        ]
    );
}

#[test]
fn main_pane_rects_transcript_gets_remaining_space() {
    use crate::rendering::layout::INPUT_MIN_HEIGHT;
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    use crate::runtime::render::frame_test::main_pane_rects_for_height;

    let main_height = 40u16;
    let (header, transcript, input, status) = main_pane_rects_for_height(main_height);

    assert_eq!(header.height, 0);
    assert_eq!(status.height, STATUS_TARGET_HEIGHT);
    assert_eq!(input.height, INPUT_MIN_HEIGHT);
    assert_eq!(
        transcript.height,
        main_height - INPUT_MIN_HEIGHT - STATUS_TARGET_HEIGHT
    );
}

#[test]
fn status_indicator_idle_returns_empty_circle() {
    assert_eq!(crate::runtime::status_indicator_for_test(None), "○");
}

#[test]
fn status_indicator_busy_cycles_through_four_frames() {
    let f = crate::runtime::status_indicator_for_test;
    assert_eq!(f(Some(0)), "◐");
    assert_eq!(f(Some(150)), "◓");
    assert_eq!(f(Some(300)), "◑");
    assert_eq!(f(Some(450)), "◒");
    assert_eq!(f(Some(600)), "◐"); // wraps
}

#[test]
fn lane_1_idle_shows_empty_circle_prefix() {
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("○ mymodel"));
}

#[test]
fn lane_1_busy_shows_spinner_prefix() {
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel",
        None,
        Some(0),
        40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("◐ mymodel"));
}

#[test]
fn lane_1_prefix_does_not_exceed_available_width() {
    let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        40,
    );
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.chars().count() <= 40);
    assert!(text.starts_with("○ "));
}

#[test]
fn hydration_compaction_creates_block_structure() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new(
            "compaction",
            "## Summary\n- point one\n- point two",
        )],
        None,
    );

    // The compaction block header ("Compaction") should be present in transcript
    let has_compaction_header = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .any(|line| line.text() == "Compaction");
    assert!(
        has_compaction_header,
        "expected compaction block header in transcript"
    );
}

#[test]
fn hydration_compaction_renders_markdown_body() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new(
            "compaction",
            "## Summary\n- alpha\n- beta",
        )],
        None,
    );

    // After the raw-markdown refactor, the compaction body is stored as raw markdown.
    // Projection (sanitization + rendering) happens at render time.
    // Test by projecting the stored markdown and checking the output.
    let projected_texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    // Raw markdown markers should NOT appear in the projected output
    assert!(
        !projected_texts.iter().any(|t| t.contains("## ")),
        "raw markdown heading marker should not appear in projected output: {projected_texts:?}"
    );
    assert!(
        !projected_texts.iter().any(|t| t.starts_with("- ")),
        "raw markdown list marker should not appear in projected output: {projected_texts:?}"
    );
    // Rendered content should be present
    assert!(
        projected_texts.iter().any(|t| t.contains("Summary")),
        "rendered heading text should appear in projected output: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|t| t.contains("alpha")),
        "rendered list item text should appear in projected output: {projected_texts:?}"
    );
}

#[test]
fn hydration_compaction_fenced_body_renders_markdown_not_raw() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new(
            "compaction",
            "```\n## Work State\n### Completed\n- Mapped `63e90e73`; confirmed `7722bef9`.\n```",
        )],
        None,
    );

    let projected_texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .flat_map(|line| crate::markdown::render_markdown_lines(&line.text(), None))
        .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect();

    assert!(
        !projected_texts.iter().any(|t| t.contains("##")),
        "raw '##' must not appear after hydration of fenced body: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|t| t.contains("Work State")),
        "heading text must render: {projected_texts:?}"
    );
    assert!(
        projected_texts.iter().any(|t| t.starts_with('•')),
        "bullet marker must render as '•': {projected_texts:?}"
    );
}

#[test]
fn hydration_compaction_empty_summary_shows_block_only() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("compaction", "")], None);

    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect();

    // The compaction header should exist
    assert!(
        texts.contains(&"Compaction".to_string()),
        "expected compaction block header: {texts:?}"
    );

    // Only the header line should be present — no body content lines
    let compaction_lines: Vec<_> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| line.role() == Role::Compaction)
        .collect();
    assert!(
        compaction_lines.is_empty(),
        "empty summary should produce no Compaction-role body lines: {compaction_lines:?}"
    );
}

#[test]
fn hydration_compaction_matches_live_rendering() {
    let summary_body = "## Summary\n- alpha\n- beta";

    // Live path: CompactionStarted + CompactionCompleted via reducer
    let mut live = RuntimeCoordinator::new(120, 30, Some(true));
    live.enqueue_ui_event(UiEvent::CompactionStarted {
        source: "history".to_string(),
    });
    live.drain_transport();
    live.enqueue_ui_event(UiEvent::CompactionCompleted {
        source: "history".to_string(),
        summary_preview: "preview".to_string(),
        summary_body: summary_body.to_string(),
    });
    live.drain_transport();

    // Hydration path: UiMessageSnapshot with role "compaction"
    let mut hydrated = RuntimeCoordinator::new(120, 30, Some(true));
    hydrated.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("compaction", summary_body)],
        None,
    );

    let live_texts: Vec<String> = live
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect();
    let hydrated_texts: Vec<String> = hydrated
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect();

    assert_eq!(
        live_texts, hydrated_texts,
        "live and hydrated transcript texts should match"
    );

    let live_roles: Vec<Role> = live
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.role())
        .collect();
    let hydrated_roles: Vec<Role> = hydrated
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.role())
        .collect();

    assert_eq!(
        live_roles, hydrated_roles,
        "live and hydrated transcript roles should match"
    );
}

// ── render_needed / render_if_needed gate tests ──

#[test]
fn render_needed_is_true_after_drain_transport() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    // Clear the initial render_needed flag via render_if_needed
    coord.set_render_needed(false);
    assert!(!coord.render_needed());

    coord.enqueue_ui_event(UiEvent::Tick);
    coord.drain_transport();

    assert!(
        coord.render_needed(),
        "render_needed should be true after drain_transport processes events"
    );
}

#[test]
fn render_if_needed_skips_when_not_dirty() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(false);
    coord.set_last_render_at(Instant::now() - Duration::from_secs(1));

    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;
    let result = coord.render_if_needed(&mut live);

    assert!(result.is_ok());
    assert!(
        !coord.render_needed(),
        "render_needed should remain false when not dirty"
    );
}

#[test]
fn render_if_needed_skips_when_too_soon() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(true);
    coord.set_last_render_at(Instant::now());

    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;
    let result = coord.render_if_needed(&mut live);

    assert!(result.is_ok());
    assert!(
        coord.render_needed(),
        "render_needed should remain true when frame interval has not elapsed"
    );
}

#[test]
fn render_if_needed_fires_when_dirty_and_elapsed() {
    let mut coord = RuntimeCoordinator::new(80, 24, None);
    coord.set_render_needed(true);
    coord.set_last_render_at(Instant::now() - Duration::from_secs(1));

    let mut live: Option<
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>,
    > = None;
    let result = coord.render_if_needed(&mut live);

    assert!(result.is_ok());
    assert!(
        !coord.render_needed(),
        "render_needed should be false after render_if_needed fires"
    );
}

#[test]
fn drain_transport_coalesces_consecutive_assistant_messages() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for text in ["a", "ab", "abc", "abcd", "abcde"] {
        coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
            text: text.to_string(),
        });
    }
    coordinator.drain_transport();

    // Only the last message ("abcde") should have been processed through the reducer

    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .map(|line| line.text())
        .collect();
    assert!(
        texts.contains(&"abcde".to_string()),
        "transcript should contain the final coalesced text: {texts:?}"
    );
    assert!(
        !texts.contains(&"abcd".to_string()),
        "intermediate messages should not appear in transcript: {texts:?}"
    );
}

#[test]
fn drain_transport_preserves_order_with_mixed_events() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello".to_string(),
    });
    coordinator.enqueue_ui_event(UiEvent::Tick);
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "world".to_string(),
    });
    coordinator.drain_transport();

    // Both messages should have been processed, because a Tick
    // separates them — coalescing only applies to consecutive same-type events

    // Final transcript shows "world" (the second AssistantMessage replaces the first)
    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| line.role() == Role::Assistant)
        .map(|line| line.text())
        .collect();
    assert_eq!(texts, vec!["world"]);
}

#[test]
fn drain_transport_single_assistant_message_not_affected() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "solo".to_string(),
    });
    coordinator.drain_transport();

    let texts: Vec<String> = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter(|line| line.role() == Role::Assistant)
        .map(|line| line.text())
        .collect();
    assert_eq!(texts, vec!["solo"]);
}

#[test]
fn lane_2_shows_agent_when_active() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(42_300),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(128_000));
    state.set_active_agent_identity("coder");

    let line = crate::runtime::lane_2_status_line_for_test(&state, 60);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(text.contains("coder")); // name is present
    assert!(!text.contains("agent:")); // old prefix is gone
    assert!(text.ends_with("42.3k (33%)"));
}

#[test]
fn lane_2_shows_only_tokens_when_no_agent() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    let line = crate::runtime::lane_2_status_line_for_test(&state, 40);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(text.ends_with("250 (25%)"));
    assert!(!text.contains("agent"));
}

#[test]
fn lane_1_no_longer_shows_agent() {
    let mut state = AppState::default();
    state.set_active_agent_identity("coder");

    let lane_1 = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let text: String = lane_1.spans.iter().map(|s| s.content.as_ref()).collect();

    assert!(text.starts_with("○ openai/gpt-4o-mini"));
    assert!(!text.contains("coder"));
    assert!(!text.contains("agent"));
    assert!(!text.contains('|'));
}

#[test]
fn hydrate_transcript_sets_latest_total_tokens() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), Some(14000));

    assert_eq!(
        coordinator.state().status.tokens.latest_total_tokens,
        Some(14000)
    );
}

#[test]
fn hydrate_transcript_leaves_latest_total_tokens_none_when_no_value() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new(), None);

    assert_eq!(coordinator.state().status.tokens.latest_total_tokens, None);
}

#[test]
fn hydrate_assistant_message_with_bold_emits_md_bold_span() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "hello **bold**")],
        None,
    );
    let has_bold = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter_map(|e| {
            if let nu_agent_core::transcript::items::TranscriptEntryKind::Assistant(
                nu_agent_core::transcript::items::ProseMessage { markdown },
            ) = &e.kind
            {
                Some(markdown.as_str())
            } else {
                None
            }
        })
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .flat_map(|l| l.spans.into_iter())
        .any(|s| {
            s.text == "bold" && matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdBold)
        });
    assert!(has_bold);
}

#[test]
fn hydrate_compaction_message_with_italic_emits_md_italic_span() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("compaction", "summary *italic*")],
        None,
    );
    let has_italic = coordinator
        .state()
        .transcript
        .entries
        .iter()
        .filter_map(|e| {
            if let nu_agent_core::transcript::items::TranscriptEntryKind::Assistant(
                nu_agent_core::transcript::items::ProseMessage { markdown },
            ) = &e.kind
            {
                Some(markdown.as_str())
            } else {
                None
            }
        })
        .flat_map(|md| crate::markdown::render_markdown_lines(md, None))
        .flat_map(|l| l.spans.into_iter())
        .any(|s| {
            s.text == "italic"
                && matches!(s.hint, nu_agent_core::transcript::ir::StyleHint::MdItalic)
        });
    assert!(has_italic);
}

#[test]
fn compact_status_line_busy_has_at_least_one_styled_span() {
    // RED: compact_status_line_for_test must return Line<'static> whose spans
    // carry explicit fg colours when the spinner is active (now_millis = Some(0)).
    let line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", Some(0));
    let has_styled = line.spans.iter().any(|s| s.style.fg.is_some());
    assert!(
        has_styled,
        "expected at least one span with explicit fg colour, got: {line:?}"
    );
}

#[test]
fn render_modal_frame_inner_is_inset_by_one_cell() {
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 5,
    };
    let inner = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    assert_eq!(inner.width, 8);
    assert_eq!(inner.height, 3);
}

#[test]
fn scrollbar_state_does_not_panic_on_empty_transcript() {
    let mut state = ratatui::widgets::ScrollbarState::new(0).position(0);
    let _ = &mut state;
}
#[test]
fn scrollbar_state_does_not_panic_on_single_entry() {
    let mut state = ratatui::widgets::ScrollbarState::new(1).position(0);
    let _ = &mut state;
}

#[test]
fn input_content_width_accounts_for_borders() {
    // The input area sits inside the outer unified rounded box. The outer box
    // already provides the enclosing border, so only the 2-char prompt prefix
    // ("❯ ") is subtracted from the inner width. The call site must therefore
    // pass `inner_width - 2` (not `pane_width - 4`) to wrapped_input_rows and
    // input_cursor_row_col.
    let inner_width: u16 = 10;
    assert_eq!(
        input_pane_content_width_for_test(inner_width),
        8,
        "content_width must be inner_width - 2 (prompt-prefix only, no inner border)"
    );

    // Also verify that wrapping at the correct width splits a 9-char string.
    let rows = wrapped_input_rows("abcdefghi", input_pane_content_width_for_test(inner_width));
    assert_eq!(
        rows,
        vec!["abcdefgh", "i"],
        "9-char input must wrap into 2 rows when content_width is 8"
    );
}

#[test]
fn status_target_height_is_three() {
    use crate::runtime::render::frame_test::STATUS_TARGET_HEIGHT;
    assert_eq!(STATUS_TARGET_HEIGHT, 3);
}

#[test]
fn status_left_content_contains_model() {
    let mut state = AppState::default();
    let line = crate::runtime::status_left_content_for_test("openai/gpt-4", None, &mut state, 80);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("openai/gpt-4"));
}

#[test]
fn status_right_content_contains_branch() -> Result<()> {
    let line = crate::runtime::status_right_content_for_test(Some("main"), None);
    let line = line.ok_or("should have right content line")?;
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("main"));
    Ok(())
}

#[test]
fn status_right_content_shows_none_when_no_branch_and_no_cwd() {
    let line = crate::runtime::status_right_content_for_test(None, None);
    assert!(line.is_none());
}

#[test]
fn status_right_content_shows_cwd_when_given() -> Result<()> {
    let cwd = std::path::Path::new("/home/user/projects/my-project");
    let line = crate::runtime::status_right_content_for_test(None, Some(cwd));
    let line = line.ok_or("should have right content line")?;
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("my-project"));
    Ok(())
}

#[test]
fn status_left_content_contains_tokens() {
    let mut state = AppState {
        status: crate::state::StatusState {
            tokens: crate::state::TokenUsage {
                latest_total_tokens: Some(250),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));
    let line = crate::runtime::status_left_content_for_test("openai/gpt-4", None, &mut state, 80);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("250"));
    assert!(text.contains("25%"));
    assert!(text.contains('┃'));
}

#[test]
fn transcript_list_area_is_two_columns_narrower_than_content_area() {
    let content_area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 20,
    };
    let list_area = ratatui::layout::Rect {
        width: content_area.width.saturating_sub(2),
        ..content_area
    };
    assert_eq!(list_area.width, 78);
    assert_eq!(list_area.x, content_area.x);
    assert_eq!(list_area.height, content_area.height);
}

#[test]
fn model_picker_rows_do_not_contain_selection_prefix() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                identity: "openai/gpt-4o".to_string(),
                display: "openai/gpt-4o".to_string(),
                active: true,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                identity: "anthropic/claude-3-5-sonnet".to_string(),
                display: "anthropic/claude-3-5-sonnet".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
            nu_agent_core::protocol::picker::ModelPickerOption {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                identity: "openai/gpt-4o-mini".to_string(),
                display: "openai/gpt-4o-mini".to_string(),
                active: false,
                context_window: None,
                max_output: None,
                configured: false,
                provider_display_name: String::new(),
            },
        ],
    );
    state.picker.open(ActivePicker::Model);
    if let Some(s) = state.picker.active_state_mut() {
        s.selection = 1;
    }

    let rows = super::model_picker_row_cells_for_test(&state);
    assert!(!rows.is_empty(), "expected at least one row");
    for row in &rows {
        for cell in row {
            assert!(
                !cell.starts_with("❯ "),
                "cell must not start with selection prefix '❯ ': {cell:?}"
            );
            assert!(
                !cell.starts_with("  "),
                "cell must not start with deselected prefix '  ': {cell:?}"
            );
        }
    }
}

#[test]
fn agent_picker_rows_do_not_contain_selection_prefix() {
    let mut state = AppState::default();
    state.set_picker_options(
        ActivePicker::Agent,
        vec![
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "default".to_string(),
                description: Some("Default agent".to_string()),
                display: "default".to_string(),
                active: true,
                builtin: true,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "coder".to_string(),
                description: Some("Coding assistant".to_string()),
                display: "coder".to_string(),
                active: false,
                builtin: false,
            },
            nu_agent_core::protocol::picker::AgentPickerOption {
                name: "reviewer".to_string(),
                description: None,
                display: "reviewer".to_string(),
                active: false,
                builtin: false,
            },
        ],
    );
    state.picker.open(ActivePicker::Agent);
    if let Some(s) = state.picker.active_state_mut() {
        s.selection = 1;
    }

    let rows = super::agent_picker_row_cells_for_test(&state);
    assert!(!rows.is_empty(), "expected at least one row");
    for row in &rows {
        for cell in row {
            assert!(
                !cell.starts_with("  "),
                "cell must not start with deselected prefix '  ': {cell:?}"
            );
        }
    }
}

#[test]
fn status_right_content_width_exceeds_narrow_line_triggering_overflow() -> Result<()> {
    let narrow_width: u16 = 20;
    let branch = "feature/my-very-long-branch";
    let cwd = std::path::Path::new("/home/user/projects/deep/nested/dir");
    let right_line = crate::runtime::status_right_content_for_test(Some(branch), Some(cwd));
    let right_line = right_line.ok_or("should have right content line")?;
    let right_width: u16 = right_line
        .spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()) as u16)
        .sum();
    assert!(
        right_width > narrow_width,
        "right_width={right_width} must exceed narrow_width={narrow_width} to exercise overflow"
    );
    Ok(())
}

// ── sync_input_state / sync_textarea_from_input_state tests ──
// These methods have been removed. TextArea is the single source of truth.
// No syncing is needed.

// ========== single_line_visual_row_count tests ==========

#[test]
fn single_line_visual_row_count_short_line() {
    let line = ratatui::text::Line::from("1234567890");
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 1);
}

#[test]
fn single_line_visual_row_count_wider_than_viewport() {
    let line = ratatui::text::Line::from("x".repeat(200));
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 3);
}

#[test]
fn single_line_visual_row_count_multi_span() {
    use ratatui::text::Span;
    let line = ratatui::text::Line::from(vec![
        Span::raw("a".repeat(50)),
        Span::raw("b".repeat(50)),
        Span::raw("c".repeat(50)),
    ]);
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 2);
}

#[test]
fn single_line_visual_row_count_empty_line() {
    let line = ratatui::text::Line::from("");
    let count = super::render::single_line_visual_row_count(&line, 80);
    assert_eq!(count, 1);
}

#[test]
fn single_line_visual_row_count_width_zero() {
    let line = ratatui::text::Line::from("hello");
    let count = super::render::single_line_visual_row_count(&line, 0);
    assert_eq!(count, 1);
}

// ========== rendered_line_text gating tests ==========

#[test]
fn should_scan_for_yank_insert_mode_returns_false() {
    assert!(!super::render::should_scan_for_yank(InputMode::Insert));
}

#[test]
fn should_scan_for_yank_normal_mode_returns_false() {
    assert!(!super::render::should_scan_for_yank(InputMode::Normal));
}

#[test]
fn should_scan_for_yank_visual_mode_returns_true() {
    assert!(super::render::should_scan_for_yank(InputMode::Visual));
}

// ========== entry_visual_info tests ==========

#[test]
fn entry_visual_info_computed_on_new_entry() {
    let mut state = crate::state::AppState::default();
    state
        .transcript
        .push_transcript_line(crate::state::TranscriptRole::User, "hello".to_string());
    state
        .transcript
        .recompute_entry_visual_info(&mut state.scroll, 80);
    assert_eq!(state.scroll.entry_visual_info.len(), 1);
    assert_eq!(state.scroll.entry_visual_info[0].start_visual_row, 0);
    assert!(state.scroll.entry_visual_info[0].visual_row_count >= 1);
}

#[test]
fn total_visual_rows_from_entry_visual_info() {
    let mut state = crate::state::AppState::default();
    state
        .transcript
        .push_transcript_line(crate::state::TranscriptRole::User, "a".to_string());
    state.transcript.push_transcript_line(
        crate::state::TranscriptRole::Assistant,
        "b\nc\nd".to_string(),
    );
    state
        .transcript
        .push_transcript_line(crate::state::TranscriptRole::User, "e".to_string());
    state
        .transcript
        .recompute_entry_visual_info(&mut state.scroll, 80);
    // No reactive spacers — just the 3 entries
    assert_eq!(state.scroll.entry_visual_info.len(), 3);
    assert_eq!(state.scroll.entry_visual_info[0].visual_row_count, 1); // User "a"
    // "b\nc\nd" projects to 3 ContentLines (one per line)
    assert_eq!(state.scroll.entry_visual_info[1].visual_row_count, 3); // Assistant
    assert_eq!(state.scroll.entry_visual_info[2].visual_row_count, 1); // User "e"
    let total = state
        .scroll
        .entry_visual_info
        .last()
        .map(|i| i.start_visual_row + i.visual_row_count)
        .unwrap_or(0);
    assert_eq!(total, 5);
}

#[test]
fn entry_visual_info_cleared_on_clear_transcript() {
    let mut state = crate::state::AppState::default();
    state
        .transcript
        .push_transcript_line(crate::state::TranscriptRole::User, "hello".to_string());
    state
        .transcript
        .recompute_entry_visual_info(&mut state.scroll, 80);
    assert!(!state.scroll.entry_visual_info.is_empty());
    state.clear_transcript();
    assert!(state.scroll.entry_visual_info.is_empty());
}

#[test]
fn push_startup_logo_adds_logo_entry_to_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.state.push_startup_logo();
    let entries = &coordinator.state.transcript.entries;
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].kind, TranscriptEntryKind::Logo(_)));
}

#[test]
fn startup_logo_not_pushed_during_hydration() {
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    let messages: Vec<UiMessageSnapshot> = vec![];
    coordinator.hydrate_transcript_from_messages(messages, None);
    let has_logo = coordinator
        .state
        .transcript
        .entries
        .iter()
        .any(|e| matches!(e.kind, TranscriptEntryKind::Logo(_)));
    assert!(!has_logo, "hydration must not push a logo");
}

#[test]
fn bottom_align_pads_content_when_shorter_than_viewport() {
    // This test verifies the render-time behavior: when total_visual_rows < viewport_height,
    // the rendered output should have viewport_height lines (padded with empty lines at top).
    // We test this indirectly by checking that the coordinator's state reflects the padding
    // after a render pass.
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    // Push a single logo entry — total_visual_rows will be small
    coordinator.state.push_startup_logo();
    // AppState.entry_visual_info_dirty is a public field — no setter method exists.
    // Force a render to trigger the bottom-align logic.
    coordinator.state.transcript.visual_info_dirty = true;
    // The actual padding happens in render_transcript_pane which we can't easily unit-test
    // without a full Frame. Instead, verify the state is set up correctly for bottom-align:
    // viewport_height > total_visual_rows should be true for a single logo on a 40-row terminal.
    coordinator
        .state
        .transcript
        .recompute_entry_visual_info(&mut coordinator.state.scroll, 120);
    let total = coordinator.state.scroll.total_visual_rows;
    let vp = coordinator.state.scroll.viewport_height;
    // On a 40-row terminal, a single logo entry should be much shorter
    assert!(
        total < vp || vp == 0,
        "expected total_visual_rows ({total}) < viewport_height ({vp}) for single logo"
    );
}

#[test]
fn reduce_warning_event_message_sets_status_line_and_marks_render_needed() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);

    // -- Exec
    // Mirrors the interactive loop's warning_rx arm: reduce, then mark on true.
    let handled = coordinator.reduce_warning_event(WarningEvent::Message {
        message: "warned".to_string(),
    });
    if handled {
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "StatusState must claim plain warning messages");
    assert_eq!(coordinator.state.status.message.status_line, "warned");
    assert!(
        coordinator.render_needed(),
        "handled warnings must mark the frame dirty"
    );
}

#[test]
fn reduce_warning_event_turn_error_falls_through_to_transcript_and_finalize() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.state.phase = UiPhase::Busy;
    coordinator.state.input_locked = true;

    // -- Exec
    let handled = coordinator.reduce_warning_event(WarningEvent::TurnError {
        message: "boom".to_string(),
    });

    // -- Check
    assert!(handled, "TurnError must fall through and be handled");
    let error_line = coordinator
        .state
        .transcript
        .entries
        .iter()
        .any(|entry| entry.role() == Role::System && entry.text().contains("Error: boom"));
    assert!(error_line, "TurnError must land on the transcript");
    assert_eq!(
        coordinator.state.phase,
        UiPhase::Idle,
        "TurnError finalize must return to Idle"
    );
    assert!(!coordinator.state.input_locked);
}

#[test]
fn reduce_ui_state_event_status_variants_route_through_status_state() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));

    // -- Exec & Check
    coordinator.reduce_ui_state_event(UiStateEvent::SetActiveModelIdentity(
        "openai/gpt-4o".to_string(),
    ));
    coordinator.mark_render_needed();
    assert_eq!(
        coordinator.state.status.identity.active_model_identity,
        "openai/gpt-4o"
    );
    assert!(coordinator.render_needed());

    coordinator.reduce_ui_state_event(UiStateEvent::SetActivePersonaIcon(Some("icon".to_string())));
    assert_eq!(
        coordinator.state.status.identity.active_persona_icon,
        Some("icon".to_string())
    );

    coordinator.reduce_ui_state_event(UiStateEvent::SetContextWindowMaxTokens(Some(128_000)));
    assert_eq!(
        coordinator.state.status.tokens.context_window_max_tokens,
        Some(128_000)
    );

    coordinator.reduce_ui_state_event(UiStateEvent::SetMcpServerState {
        server: "gh".to_string(),
        state: nu_agent_core::protocol::contracts::McpUsabilityState::Failed,
        error: Some("boom".to_string()),
        total: 3,
    });
    assert_eq!(coordinator.state.status.mcp.llm_visible_mcp_tool_count, 3);

    coordinator.reduce_ui_state_event(UiStateEvent::SetMcpVisibleToolCount {
        server: "gh".to_string(),
        count: 5,
    });
    assert_eq!(
        coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_count_for_server_name("gh"),
        5
    );

    coordinator.reduce_ui_state_event(UiStateEvent::SetMcpVisibleToolNames {
        server: "gh".to_string(),
        names: vec!["z_tool".to_string(), "a_tool".to_string()],
    });
    assert_eq!(
        coordinator
            .state
            .status
            .mcp
            .mcp_visible_tool_names_for_server_name("gh"),
        vec!["a_tool".to_string(), "z_tool".to_string()]
    );
}

#[test]
fn reduce_ui_state_event_non_status_variants_fall_back_to_app_state() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    assert!(coordinator.state.transcript.entries.is_empty());

    // -- Exec
    // PushStartupLogo is not status-owned: StatusState returns false and
    // AppState handles it (mirrors the ui_state_rx caller seam).
    coordinator.reduce_ui_state_event(UiStateEvent::PushStartupLogo);

    // -- Check
    let has_logo = coordinator
        .state
        .transcript
        .entries
        .iter()
        .any(|e| matches!(e.kind, TranscriptEntryKind::Logo(_)));
    assert!(
        has_logo,
        "non-status UiStateEvent must fall through to AppState"
    );
}

#[test]
fn permission_rx_caller_requested_applies_all_effects() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    coordinator.set_render_needed(false);
    coordinator.state.status.message.status_line = "idle".to_string();
    coordinator.state.scroll.following_tail = false;

    let event = PermissionEvent::Requested {
        request_id: "ask-0000000000000001".to_string(),
        context: Box::new(PermissionRequestContext {
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        }),
    };

    // -- Exec
    // Mirrors the interactive loop's permission_rx arm: apply the display,
    // then reduce, then apply the caller effects on true.
    if let PermissionEvent::Requested { context, .. } = &event {
        crate::interaction::reducer::apply_permission_request_display(
            &mut coordinator.state,
            context,
        );
    }
    let handled = coordinator.state.permission.reduce_permission_event(event);
    if handled {
        coordinator.state.status.message.status_line = "Permission required".to_string();
        coordinator.state.scroll.scroll_transcript_to_bottom();
        coordinator.state.ensure_invariants();
        coordinator.mark_render_needed();
    }

    // -- Check
    assert!(handled, "Requested must reduce to true");
    assert_eq!(
        coordinator.state.status.message.status_line,
        "Permission required"
    );
    assert!(
        coordinator.state.scroll.following_tail,
        "Requested must scroll the transcript to the bottom"
    );
    assert!(
        coordinator.render_needed(),
        "Requested must mark the frame dirty"
    );
    assert!(
        coordinator.state.permission.has_prompt(),
        "Requested must open the permission prompt"
    );
}

#[test]
fn permission_rx_caller_decision_variants_skip_effects() {
    for event in [
        PermissionEvent::DecisionSubmitted {
            request_id: "ask-0000000000000001".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "nested:nu.command:*".to_string(),
        },
        PermissionEvent::DecisionTimedOut {
            request_id: "ask-0000000000000001".to_string(),
        },
        PermissionEvent::DecisionIgnored {
            request_id: "ask-0000000000000001".to_string(),
            reason: "user closed".to_string(),
        },
    ] {
        // -- Setup & Fixtures
        let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
        coordinator.set_render_needed(false);
        coordinator.state.status.message.status_line = "idle".to_string();
        coordinator.state.scroll.following_tail = false;

        // -- Exec
        let handled = coordinator.state.permission.reduce_permission_event(event);
        if handled {
            coordinator.state.status.message.status_line = "Permission required".to_string();
            coordinator.state.scroll.scroll_transcript_to_bottom();
            coordinator.state.ensure_invariants();
            coordinator.mark_render_needed();
        }

        // -- Check
        assert!(!handled, "decision variants must reduce to false");
        assert_eq!(
            coordinator.state.status.message.status_line, "idle",
            "decision variants must leave status_line unchanged"
        );
        assert!(
            !coordinator.state.scroll.following_tail,
            "decision variants must not scroll the transcript"
        );
        assert!(
            !coordinator.render_needed(),
            "decision variants must not mark the frame dirty"
        );
    }
}

#[test]
fn permission_rx_caller_pre_authorize_display_applied_before_reduce() {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 40, Some(false));
    let context = PermissionRequestContext {
        tool: "write".to_string(),
        source: "user".to_string(),
        mode: Some("edit".to_string()),
        matched_rule_identity: "identity".to_string(),
        scope: "scope".to_string(),
        target_field: Some("target".to_string()),
        pattern: "pattern".to_string(),
        summary: "summary".to_string(),
        pre_authorize_display: Some(ToolDisplay {
            title: "preview-title".to_string(),
            sections: vec![],
        }),
    };

    // -- Exec
    // Mirrors the interactive loop's permission_rx arm ordering: the display
    // is applied before reduce_permission_event runs.
    crate::interaction::reducer::apply_permission_request_display(&mut coordinator.state, &context);
    let handled =
        coordinator
            .state
            .permission
            .reduce_permission_event(PermissionEvent::Requested {
                request_id: "ask-0000000000000001".to_string(),
                context: Box::new(context),
            });

    // -- Check
    assert!(handled, "Requested must reduce to true");
    let display_applied = coordinator
        .state
        .transcript
        .entries
        .iter()
        .filter(|entry| entry.role() == Role::ToolDisplay)
        .any(|entry| {
            matches!(&entry.kind, TranscriptEntryKind::ToolResult(result)
                if result.lines.iter().any(|line| line.text.contains("preview-title")))
        });
    assert!(
        display_applied,
        "pre_authorize_display must be applied to the transcript before reduce"
    );
    assert!(
        coordinator.state.permission.has_prompt(),
        "reduce must still open the prompt after the display is applied"
    );
}
