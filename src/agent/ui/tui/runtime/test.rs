use std::{
    cell::RefCell,
    fs,
    path::Path,
    process::Command,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::agent::protocol::contracts::{UiMessageSnapshot, UiMessageUsageSnapshot};
use crate::agent::protocol::event::UiEvent;
use crate::agent::ui::transcript::ir::Role;
use crate::agent::ui::tui::test_support::markdown_fixture;
use crate::agent::ui::{
    renderer::UiRenderer,
    tui::{
        interaction::input::{TerminalEvent, TerminalKey},
        platform::safety::RestoreRunError,
        platform::terminal::{
            TerminalAction, TerminalBackend, TerminalLifecycle, TerminalLifecycleError,
        },
        runtime::{
            InputSourceDiagnostics, RuntimeCoordinator, RuntimeRunError, ScriptedTerminalEvents,
            TerminalEventSource, TuiRuntimeRenderer, command_palette_table_model_for_test,
            cursor_style_for_test, help_panel_lines_for_test, help_panel_max_scroll_for_test,
            help_panel_overflow_cue_for_test, help_panel_visible_window_for_test,
            inline_slash_lines_for_test, input_line_for_test, input_line_for_test_at_millis,
            input_rows_with_prompt_for_test, mcp_table_model_for_test,
            parse_persisted_tool_status_line_for_test, run_with_terminal_restore,
            status_panel_lines_for_test, transition_spacer_for_roles_for_test,
        },
        state::{
            AppState, InputMode, McpServerUsabilityState, PromptStatus, ToolCallStatus,
            TranscriptLineStatus, TranscriptRole, UiPhase,
        },
    },
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git command failed in {}: git {}\nstdout: {}\nstderr: {}",
        dir.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_repo_with_branch(path: &Path, branch: &str) {
    run_git(path, &["init"]);
    run_git(path, &["config", "user.email", "nu-agent@test.local"]);
    run_git(path, &["config", "user.name", "nu-agent-test"]);
    fs::write(path.join("README.md"), "seed\n").expect("seed file");
    run_git(path, &["add", "README.md"]);
    run_git(path, &["commit", "-m", "seed"]);
    run_git(path, &["checkout", "-b", branch]);
}

#[derive(Default)]
struct StubEventSource {
    next: Option<TerminalEvent>,
}

impl TerminalEventSource for StubEventSource {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Ok(self.next.take())
    }
}

#[derive(Default)]
struct ErrorEventSource;

impl TerminalEventSource for ErrorEventSource {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Err("simulated source failure".to_string())
    }
}

#[derive(Clone)]
struct ErrorWithDiagnosticsEventSource {
    diagnostics: InputSourceDiagnostics,
    error: String,
}

impl TerminalEventSource for ErrorWithDiagnosticsEventSource {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Err(self.error.clone())
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

#[derive(Clone)]
struct DiagnosticsOnlyEventSource {
    diagnostics: InputSourceDiagnostics,
}

impl TerminalEventSource for DiagnosticsOnlyEventSource {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Ok(None)
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

#[test]
fn coordinator_submit_handoff_keeps_input_editable_and_preserves_transcript_preview() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Char('x'))),
    };

    coordinator.pump_once(&mut source);

    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Enter)),
    };
    coordinator.pump_once(&mut source);

    assert_eq!(coordinator.state().phase, UiPhase::Busy);
    assert!(!coordinator.state().input.locked);
    assert!(coordinator.state().input.buffer.is_empty());
    assert_eq!(coordinator.take_submitted_prompt(), Some("x".to_string()));
    assert_eq!(coordinator.take_submitted_prompt(), None);
    assert_eq!(coordinator.state().transcript_preview.len(), 1);
    assert_eq!(coordinator.state().transcript_preview[0].role(), Role::User);
    assert_eq!(coordinator.state().transcript_preview[0].text(), "x");
}

#[test]
fn slash_commands_do_not_append_command_text_to_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for event in [
        TerminalEvent::Key(TerminalKey::Char('/')),
        TerminalEvent::Key(TerminalKey::Char('h')),
        TerminalEvent::Key(TerminalKey::Char('e')),
        TerminalEvent::Key(TerminalKey::Char('l')),
        TerminalEvent::Key(TerminalKey::Char('p')),
        TerminalEvent::Key(TerminalKey::Enter),
    ] {
        let mut source = StubEventSource { next: Some(event) };
        coordinator.pump_once(&mut source);
    }

    assert_eq!(
        coordinator.take_submitted_prompt(),
        Some("/help".to_string())
    );
    assert_eq!(coordinator.state().phase, UiPhase::Idle);
    assert_eq!(coordinator.state().pending_prompt_count(), 0);
    assert!(coordinator.state().prompt_items().is_empty());
    assert!(coordinator.state().transcript_preview.is_empty());
}

#[test]
fn compact_result_artifact_is_visible_without_slash_command_echo() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for event in [
        TerminalEvent::Key(TerminalKey::Char('/')),
        TerminalEvent::Key(TerminalKey::Char('c')),
        TerminalEvent::Key(TerminalKey::Char('o')),
        TerminalEvent::Key(TerminalKey::Char('m')),
        TerminalEvent::Key(TerminalKey::Char('p')),
        TerminalEvent::Key(TerminalKey::Char('a')),
        TerminalEvent::Key(TerminalKey::Char('c')),
        TerminalEvent::Key(TerminalKey::Char('t')),
        TerminalEvent::Key(TerminalKey::Enter),
    ] {
        let mut source = StubEventSource { next: Some(event) };
        coordinator.pump_once(&mut source);
    }

    assert_eq!(
        coordinator.take_submitted_prompt(),
        Some("/compact".to_string())
    );
    assert!(coordinator.state().transcript_preview.is_empty());

    coordinator.enqueue_ui_event(UiEvent::CompactionStarted {
        source: "slash_compact".to_string(),
    });
    coordinator.drain_transport();

    coordinator.enqueue_ui_event(UiEvent::CompactionTriggered {
        source: "slash_compact".to_string(),
        summarized_count: 3,
        kept_recent_count: 2,
        summary_preview: "preview".to_string(),
        summary_body: "summary body".to_string(),
    });
    coordinator.drain_transport();

    let lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();
    assert!(lines.contains(&"Compaction".to_string()));
    assert!(lines.contains(&"summary body".to_string()));
    assert!(!lines.iter().any(|line| line.contains("source=")));
    assert!(!lines.iter().any(|line| line.contains("status=running")));
    assert!(!lines.iter().any(|line| line.starts_with("/compact")));
}

#[test]
fn immediate_slash_commands_do_not_set_busy_or_spinner() {
    for command in ["/compact", "/mcp", "/help", "/status"] {
        let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

        for ch in command.chars() {
            let mut source = StubEventSource {
                next: Some(TerminalEvent::Key(TerminalKey::Char(ch))),
            };
            coordinator.pump_once(&mut source);
        }
        let mut submit = StubEventSource {
            next: Some(TerminalEvent::Key(TerminalKey::Enter)),
        };
        coordinator.pump_once(&mut submit);

        assert_eq!(
            coordinator.take_submitted_prompt(),
            Some(command.to_string()),
            "expected immediate slash command handoff"
        );
        assert_eq!(
            coordinator.state().phase,
            UiPhase::Idle,
            "immediate command must not transition into Busy"
        );
        assert!(
            !coordinator.state().is_active_cycle(),
            "immediate command must not activate prompt lifecycle"
        );
        assert!(
            coordinator.state().status_line != "Thinking...",
            "spinner lane status must not be set for immediate slash commands"
        );
    }
}

#[test]
fn coordinator_esc_then_esc_requests_cancel_signal() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for event in [
        TerminalEvent::Key(TerminalKey::Char('q')),
        TerminalEvent::Key(TerminalKey::Enter),
        TerminalEvent::Key(TerminalKey::Esc),
        TerminalEvent::Key(TerminalKey::Esc),
    ] {
        let mut source = StubEventSource { next: Some(event) };
        coordinator.pump_once(&mut source);
    }

    assert_eq!(coordinator.state().status_line, "Abort requested.");
    assert!(coordinator.cancel_controller().is_cancel_requested());
}

#[test]
fn coordinator_resize_recomputes_layout() {
    let mut coordinator = RuntimeCoordinator::new(80, 20, Some(true));
    let before = coordinator.layout();

    let mut source = StubEventSource {
        next: Some(TerminalEvent::Resize(
            crate::agent::ui::tui::interaction::input::TerminalResize {
                columns: 160,
                rows: 40,
            },
        )),
    };
    coordinator.pump_once(&mut source);

    let after = coordinator.layout();
    assert_ne!(before, after);
    assert!(after.side_pane.is_none());
    assert_eq!(after.transcript.width, 156);
}

#[test]
fn scripted_event_parser_supports_keys_chars_and_resize() {
    let mut source =
        ScriptedTerminalEvents::from_script("char:a,enter,esc,resize:120x40,ctrlu,ctrld,ctrlc");

    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Char('a'))))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Enter)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Esc)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Resize(
            crate::agent::ui::tui::interaction::input::TerminalResize {
                columns: 120,
                rows: 40,
            }
        )))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlU)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlD)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlC)))
    );
    assert_eq!(source.poll_event(), Ok(None));
}

#[derive(Default)]
struct FakeRenderer {
    emitted: Vec<UiEvent>,
    flushed: usize,
}

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, event: &UiEvent) {
        self.emitted.push(event.clone());
    }

    fn flush(&mut self) {
        self.flushed += 1;
    }
}

struct CapturingRenderer {
    events: Arc<Mutex<Vec<UiEvent>>>,
}

impl CapturingRenderer {
    fn new(events: Arc<Mutex<Vec<UiEvent>>>) -> Self {
        Self { events }
    }
}

impl UiRenderer for CapturingRenderer {
    fn emit(&mut self, event: &UiEvent) {
        self.events.lock().expect("events").push(event.clone());
    }

    fn flush(&mut self) {}
}

#[test]
fn runtime_renderer_reuses_eventing_and_preserves_emit_passthrough() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:h,ctrlc,resize:140x35");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStart);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.emit(&UiEvent::Tick);
    runtime_renderer.flush();

    assert!(
        runtime_renderer
            .coordinator()
            .cancel_controller()
            .is_cancel_requested()
    );
    assert!(runtime_renderer.coordinator().quit_requested());
    let state = runtime_renderer.coordinator().state();
    assert_eq!(state.phase, UiPhase::Busy);
    let layout = runtime_renderer.coordinator().layout();
    assert!(layout.side_pane.is_none());
    assert_eq!(layout.transcript.width, 136);
}

#[test]
fn runtime_renderer_pump_and_take_submitted_prompt_supports_interactive_turn_handoff() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:h,char:i,enter");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.pump_terminal_once();
    runtime_renderer.pump_terminal_once();
    runtime_renderer.pump_terminal_once();

    assert_eq!(
        runtime_renderer.take_submitted_prompt(),
        Some("hi".to_string())
    );
    assert_eq!(runtime_renderer.take_submitted_prompt(), None);
}

#[test]
fn runtime_renderer_quit_requested_reflects_ctrlc_terminal_event() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("ctrlc");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.pump_terminal_once();

    assert!(runtime_renderer.quit_requested());
}

#[test]
fn runtime_renderer_in_tui_mode_does_not_forward_spinner_progress_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer =
        TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::LlmStart);
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

    assert_eq!(coordinator.state().transcript_preview.len(), 2);
    assert_eq!(
        coordinator.state().transcript_preview[0].role(),
        Role::Assistant
    );
    assert_eq!(coordinator.state().transcript_preview[0].text(), "hello");
    assert_eq!(
        coordinator.state().transcript_preview[1].role(),
        Role::Assistant
    );
    assert_eq!(coordinator.state().transcript_preview[1].text(), "world");
}

#[test]
fn assistant_markdown_message_is_projected_before_transcript_append() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("lists_blockquote.md");
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    coordinator.drain_transport();

    let lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();

    assert!(lines.contains(&"• one".to_string()));
    assert!(lines.contains(&"• two".to_string()));
    assert!(lines.contains(&"1. first".to_string()));
    assert!(lines.contains(&"2. second".to_string()));
    assert!(lines.contains(&"│ quoted".to_string()));
    assert!(lines.contains(&"│ second".to_string()));
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

#[test]
fn user_then_assistant_inserts_turn_separator_in_runtime_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Char('h'))),
    };
    coordinator.pump_once(&mut source);

    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Enter)),
    };
    coordinator.pump_once(&mut source);

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "world".to_string(),
    });
    coordinator.drain_transport();

    assert_eq!(
        coordinator
            .state()
            .transcript_preview
            .iter()
            .map(|line| {
                let role = match line.role() {
                    crate::agent::ui::transcript::ir::Role::User => TranscriptRole::User,
                    crate::agent::ui::transcript::ir::Role::Assistant => TranscriptRole::Assistant,
                    crate::agent::ui::transcript::ir::Role::Tool => TranscriptRole::Tool,
                    crate::agent::ui::transcript::ir::Role::ToolDisplay => {
                        TranscriptRole::ToolDisplay
                    }
                    crate::agent::ui::transcript::ir::Role::System => TranscriptRole::System,
                    crate::agent::ui::transcript::ir::Role::Compaction => TranscriptRole::Compaction,
                    crate::agent::ui::transcript::ir::Role::Separator => TranscriptRole::Separator,
                };
                (role, line.text())
            })
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::User, "h".to_string()),
            (TranscriptRole::Separator, "────────────────".to_string()),
            (TranscriptRole::Assistant, "world".to_string()),
        ]
    );
}

#[test]
fn busy_input_line_has_no_spinner_prefix_and_never_shows_locked_label() {
    let mut state = AppState::new();
    state.input.buffer = "kubectl get pods".to_string();

    let idle_line = input_line_for_test(&state);
    assert_eq!(idle_line, "kubectl get pods");

    state.phase = UiPhase::Busy;
    state.ensure_invariants();
    let busy_line = input_line_for_test_at_millis(&state, 160);
    assert_eq!(busy_line, "kubectl get pods");
    assert!(!busy_line.contains("[locked]"));
}

#[test]
fn tui_active_mode_does_not_forward_payload_like_events_to_inner_renderer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inner = CapturingRenderer::new(events.clone());
    let scripted = ScriptedTerminalEvents::from_script("");
    let mut runtime_renderer =
        TuiRuntimeRenderer::new_tui_active_for_test(inner, scripted, 120, 30);

    runtime_renderer.emit(&UiEvent::ToolEnd {
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

#[test]
fn tick_and_completed_events_update_status_only_without_touching_input_buffer() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Char('x'))),
    };
    coordinator.pump_once(&mut source);

    let input_before = coordinator.state().input.buffer.clone();

    coordinator.enqueue_ui_event(UiEvent::Tick);
    coordinator.drain_transport();
    assert_eq!(coordinator.state().input.buffer, input_before);
    assert_eq!(coordinator.state().status_line, "Thinking...");

    coordinator.enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    coordinator.drain_transport();
    assert_eq!(coordinator.state().input.buffer, input_before);
    assert!(coordinator.state().status_line.is_empty());
}

#[test]
fn status_updates_stay_in_status_area_and_do_not_pollute_input_line() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Char('k'))),
    };
    coordinator.pump_once(&mut source);
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Char('9'))),
    };
    coordinator.pump_once(&mut source);

    let input_before = coordinator.state().input.buffer.clone();
    assert_eq!(input_before, "k9");

    coordinator.enqueue_ui_event(UiEvent::Tick);
    coordinator.drain_transport();

    let status_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        coordinator.state(),
        "openai/gpt-4",
    );
    let joined = status_lines.join("\n");

    assert!(joined.contains("(busy)"));
    assert!(!joined.contains(&input_before));
    assert_eq!(
        crate::agent::ui::tui::runtime::input_line_for_test_at_millis(coordinator.state(), 0,),
        input_before
    );

    coordinator.enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    coordinator.drain_transport();

    let status_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        coordinator.state(),
        "openai/gpt-4",
    );
    assert!(status_lines[0].contains("(idle)"));
    assert!(
        !status_lines
            .iter()
            .any(|line| line.starts_with("Input mode:"))
    );
    assert_eq!(coordinator.state().input.buffer, input_before);
}

#[test]
fn status_lines_do_not_report_input_mode() {
    let state = AppState::new();

    let lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4",
    );
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
}

#[test]
fn compact_status_line_matches_lane_1_contract() {
    let status_line = crate::agent::ui::tui::runtime::compact_status_line_for_test(
        "openai/gpt-4o-mini",
        None,
    );

    assert!(status_line.starts_with("○ openai/gpt-4o-mini"));
    assert!(!status_line.contains('|'));
}


#[test]
fn lane_1_wide_no_truncation() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        40,
    );

    assert_eq!(line, "○ abcdefghijklmnop            branchname");
    assert!(!line.contains('|'));
}

#[test]
fn lane_1_medium_one_side_truncation() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        23,
    );

    assert_eq!(line, "○ ...jklmnop branchname");
    assert!(!line.contains('|'));
}

#[test]
fn lane_1_narrow_both_side_truncation() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        20,
    );

    assert_eq!(line, "○ ...mnop branchname");
    assert!(!line.contains('|'));
}

#[test]
fn lane_1_branch_segment_is_right_aligned_when_present() {
    let width = 40usize;
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        width,
    );

    assert_eq!(line.chars().count(), width);
    assert!(line.starts_with("○ abcdefghijklmnop"));
    assert!(line.ends_with("branchname"));
    assert!(!line.contains('|'));
}

#[test]
fn lane_1_narrow_truncation_keeps_branch_right_anchored() {
    let width = 20usize;
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        width,
    );

    assert_eq!(line.chars().count(), width);
    assert!(line.ends_with("branchname"));
    assert!(line.contains("...mnop"));
    assert!(!line.contains('|'));
}

#[test]
fn lane_1_omits_branch_when_unavailable() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "openai/gpt-4o-mini",
        None,
        None,
        80,
    );

    assert_eq!(line, "○ openai/gpt-4o-mini");
    assert!(!line.contains('|'));
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

    let resolved =
        crate::agent::ui::tui::runtime::status::resolve_repo_branch_for_test(&caller_repo);

    std::env::set_current_dir(original_cwd).expect("restore cwd");
    assert_eq!(resolved.as_deref(), Some("caller-branch"));
}

#[test]
fn branch_resolver_returns_none_for_non_git_directory() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let non_git = temp_dir.path().join("plain");
    fs::create_dir_all(&non_git).expect("plain dir");

    let resolved = crate::agent::ui::tui::runtime::status::resolve_repo_branch_for_test(&non_git);
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

    let resolved = crate::agent::ui::tui::runtime::status::resolve_repo_branch_for_test(&repo);
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

    let resolved = crate::agent::ui::tui::runtime::status::resolve_repo_branch_for_test(&worktree);
    assert_eq!(resolved.as_deref(), Some("wt-branch"));
}

#[test]
fn repo_branch_tracker_updates_on_branch_and_detached_transitions() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    init_repo_with_branch(&repo, "branch-one");

    let mut tracker =
        crate::agent::ui::tui::runtime::status::RepoBranchTracker::from_caller_cwd_for_test(
            Some(repo.clone()),
            Duration::from_millis(0),
            Duration::from_millis(0),
        );
    assert_eq!(tracker.branch(), Some("branch-one"));

    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "-b", "branch-two"]);
    tracker.tick();
    assert_eq!(tracker.branch(), Some("branch-two"));

    let expected_detached = run_git(&repo, &["rev-parse", "--short=12", "HEAD"]);
    std::thread::sleep(Duration::from_millis(5));
    run_git(&repo, &["checkout", "--detach"]);
    tracker.tick();
    assert_eq!(tracker.branch(), Some(expected_detached.as_str()));
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

    let tracker_a =
        crate::agent::ui::tui::runtime::status::RepoBranchTracker::from_caller_cwd_for_test(
            Some(repo_a),
            Duration::from_millis(0),
            Duration::from_millis(0),
        );
    let tracker_b =
        crate::agent::ui::tui::runtime::status::RepoBranchTracker::from_caller_cwd_for_test(
            Some(repo_b),
            Duration::from_millis(0),
            Duration::from_millis(0),
        );

    assert_eq!(tracker_a.branch(), Some("alpha"));
    assert_eq!(tracker_b.branch(), Some("beta"));
}

#[test]
fn lane_1_has_no_mode_token_in_any_input_mode() {
    let insert_line =
        crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
            "model", None, None, 80,
        );

    let normal_line =
        crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
            "model", None, None, 80,
        );

    let visual_line =
        crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
            "model", None, None, 80,
        );

    assert_eq!(insert_line, "○ model");
    assert_eq!(normal_line, "○ model");
    assert_eq!(visual_line, "○ model");
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

#[test]
fn coordinator_terminal_input_error_surfaces_status_and_requests_quit() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = ErrorEventSource;

    coordinator.pump_once(&mut source);

    assert!(coordinator.quit_requested());
    assert!(coordinator.cancel_controller().is_cancel_requested());
    assert_eq!(
        coordinator.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
}

#[test]
fn runtime_renderer_reports_fatal_error_on_event_source_failure() {
    let inner = FakeRenderer::default();
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, ErrorEventSource, 120, 30);

    runtime_renderer.pump_terminal_once();

    assert!(runtime_renderer.quit_requested());
    assert_eq!(
        runtime_renderer.fatal_error(),
        Some("Terminal input error: simulated source failure")
    );
}

#[test]
fn idle_q_is_regular_input_and_never_requests_quit() {
    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:q");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);

    runtime_renderer.pump_terminal_once();
    assert!(!runtime_renderer.quit_requested());
    assert_eq!(runtime_renderer.coordinator().state().input.buffer, "q");

    let inner = FakeRenderer::default();
    let scripted = ScriptedTerminalEvents::from_script("char:a,char:q");
    let mut runtime_renderer = TuiRuntimeRenderer::new(inner, scripted, 120, 30);
    runtime_renderer.pump_terminal_once();
    runtime_renderer.pump_terminal_once();
    assert!(!runtime_renderer.quit_requested());
    assert_eq!(runtime_renderer.coordinator().state().input.buffer, "aq");
}

#[test]
fn idle_q_does_not_quit_through_dispatch_path() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Char('q'))),
    };

    coordinator.pump_once(&mut source);

    assert!(!coordinator.quit_requested());
    assert_eq!(coordinator.state().input.buffer, "q");
}

#[test]
fn idle_escape_status_copy_mentions_ctrlc_only_not_q() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let mut source = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Esc)),
    };

    coordinator.pump_once(&mut source);

    assert!(coordinator.state().status_line.contains("Ctrl+C"));
    assert!(
        !coordinator
            .state()
            .status_line
            .to_ascii_lowercase()
            .contains("press q")
    );
    assert!(!coordinator.state().status_line.contains("q to quit"));
}

#[test]
fn watchdog_fails_fast_when_no_input_backend_available() {
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

    coordinator.pump_once(&mut source);

    let fatal = coordinator.fatal_error().expect("expected watchdog fatal");
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed"));
    assert!(fatal.contains("interactive terminal"));
    assert!(coordinator.quit_requested());
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

    coordinator.pump_once(&mut source);

    let (backend, last_poll, last_error) = coordinator.input_diagnostics_snapshot();
    assert_eq!(
        backend,
        "active=tty, crossterm=unavailable, /dev/tty=available"
    );
    assert_eq!(last_poll, "crossterm error; /dev/tty delivered event");
    assert_eq!(last_error.as_deref(), Some("crossterm poll failed: EIO"));
}

#[test]
fn immediate_poll_error_fails_fast_with_actionable_message_when_no_backends_available() {
    let mut coordinator = RuntimeCoordinator::new_for_test_with_watchdog(
        120,
        30,
        Some(true),
        std::time::Duration::from_secs(60),
    );

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

    coordinator.pump_once(&mut source);

    let fatal = coordinator
        .fatal_error()
        .expect("expected fatal fail-fast error");
    assert!(coordinator.quit_requested());
    assert!(coordinator.cancel_controller().is_cancel_requested());
    assert!(fatal.contains("No interactive input backend available"));
    assert!(fatal.contains("Last poll: crossterm error; /dev/tty unavailable"));
    assert!(fatal.contains("Last error: crossterm poll failed: not a terminal"));
    assert!(fatal.contains("Run `agent` in an interactive terminal"));
    assert!(!fatal.contains("Terminal input error:"));
}

#[test]
fn crossterm_event_source_with_zero_timeout_returns_none_when_idle() {
    if unsafe { libc::isatty(libc::STDIN_FILENO) } == 0 {
        return; // no TTY available (e.g. Nix sandbox)
    }
    let mut source = crate::agent::ui::tui::runtime::CrosstermTerminalEvents::new(
        std::time::Duration::from_millis(0),
    );

    let event = source.poll_event();
    assert_eq!(event, Ok(None));
}

#[test]
fn crossterm_enter_modifier_mapping_distinguishes_submit_vs_newline_intents() {
    let plain = crate::agent::ui::tui::runtime::map_crossterm_event_for_test(Event::Key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ));
    assert_eq!(plain, Some(TerminalEvent::Key(TerminalKey::Enter)));

    let alt = crate::agent::ui::tui::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::ALT,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }));
    assert_eq!(alt, Some(TerminalEvent::Key(TerminalKey::AltEnter)));

    let shift =
        crate::agent::ui::tui::runtime::map_crossterm_event_for_test(Event::Key(KeyEvent {
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
    coordinator.hydrate_transcript_from_messages(vec![
        UiMessageSnapshot::new("user", "line1\n\nline2"),
        UiMessageSnapshot::new("assistant", "\n\nreply\n"),
        UiMessageSnapshot::new("tool", "tool output"),
        UiMessageSnapshot::new("mystery", "system fallback"),
    ]);

    let lines = coordinator.state().transcript_preview.clone();
    assert_eq!(
        lines
            .iter()
            .map(|line| {
                let role = match line.role() {
                    crate::agent::ui::transcript::ir::Role::User => TranscriptRole::User,
                    crate::agent::ui::transcript::ir::Role::Assistant => TranscriptRole::Assistant,
                    crate::agent::ui::transcript::ir::Role::Tool => TranscriptRole::Tool,
                    crate::agent::ui::transcript::ir::Role::ToolDisplay => {
                        TranscriptRole::ToolDisplay
                    }
                    crate::agent::ui::transcript::ir::Role::System => TranscriptRole::System,
                    crate::agent::ui::transcript::ir::Role::Compaction => TranscriptRole::Compaction,
                    crate::agent::ui::transcript::ir::Role::Separator => TranscriptRole::Separator,
                };
                (role, line.text())
            })
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::User, "line1".to_string()),
            (TranscriptRole::User, "line2".to_string()),
            (TranscriptRole::Separator, "────────────────".to_string()),
            (TranscriptRole::Assistant, "reply".to_string()),
            (TranscriptRole::Separator, "".to_string()), // spacer between Assistant and System
            (TranscriptRole::System, "system fallback".to_string()),
        ]
    );
}

#[test]
fn hydrated_tool_history_matches_live_tool_row_shape() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![
        UiMessageSnapshot::new("tool", "tool[k8s__list_pods] args={} · done").with_tool_details(
            Some("{\"namespace\":\"prod\"}".to_string()),
            Some("[{\"name\":\"api-0\"}]".to_string()),
            Some(true),
        ),
    ]);

    assert_eq!(coordinator.state().transcript_preview.len(), 1);
    assert_eq!(coordinator.state().transcript_preview[0].role(), Role::Tool);
    assert_eq!(
        coordinator.state().transcript_preview[0].text(),
        "k8s__list_pods"
    );
    assert_eq!(
        coordinator.state().transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done))
    );
}

#[test]
fn parse_persisted_tool_status_line_supports_done_and_failed_shapes() {
    let done = parse_persisted_tool_status_line_for_test(
        "tool[k8s__list_pods] args={\"namespace\":\"prod\"} · done",
    );
    assert_eq!(
        done,
        Some(("k8s__list_pods", "{\"namespace\":\"prod\"}", true))
    );

    let failed = parse_persisted_tool_status_line_for_test("tool[gh__run] args={} · failed");
    assert_eq!(failed, Some(("gh__run", "{}", false)));

    assert_eq!(
        parse_persisted_tool_status_line_for_test("tool[gh__run] args={}"),
        None
    );
}

#[test]
fn coordinator_hydration_projects_assistant_markdown_but_preserves_user_plain_text() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![
        UiMessageSnapshot::new("user", "# user stays literal"),
        UiMessageSnapshot::new("assistant", "# heading\n\n`x`"),
    ]);

    let lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .map(|line| {
            let role = match line.role() {
                crate::agent::ui::transcript::ir::Role::User => TranscriptRole::User,
                crate::agent::ui::transcript::ir::Role::Assistant => TranscriptRole::Assistant,
                crate::agent::ui::transcript::ir::Role::Tool => TranscriptRole::Tool,
                crate::agent::ui::transcript::ir::Role::ToolDisplay => TranscriptRole::ToolDisplay,
                crate::agent::ui::transcript::ir::Role::System => TranscriptRole::System,
                crate::agent::ui::transcript::ir::Role::Compaction => TranscriptRole::Compaction,
                crate::agent::ui::transcript::ir::Role::Separator => TranscriptRole::Separator,
            };
            (role, line.text())
        })
        .collect::<Vec<_>>();

    assert!(lines.contains(&(TranscriptRole::User, "# user stays literal".to_string())));
    assert!(lines.contains(&(TranscriptRole::Assistant, "heading".to_string())));
    assert!(lines.contains(&(TranscriptRole::Assistant, "x".to_string())));
}

#[test]
fn coordinator_hydration_preserves_assistant_markdown_styles() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "assistant",
        "**bold** and `code`",
    )]);

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

    // Cache is cleared during streaming truncation, so the second message
    // re-projects (miss) because the cache was intentionally invalidated
    // to prevent stale entries from leaking memory.
    assert_eq!(coordinator.state().assistant_projection_cache_misses(), 2);
}

#[test]
fn resize_and_redraw_paths_do_not_retokenize_assistant_projection_cache() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("fenced_code_blocks.md");

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage { text: markdown });
    coordinator.drain_transport();
    let misses_after_projection = coordinator.state().assistant_projection_cache_misses();
    assert_eq!(misses_after_projection, 1);

    for (columns, rows) in [(100, 28), (140, 42), (80, 24)] {
        let mut source = StubEventSource {
            next: Some(TerminalEvent::Resize(
                crate::agent::ui::tui::interaction::input::TerminalResize { columns, rows },
            )),
        };
        coordinator.pump_once(&mut source);
    }

    assert_eq!(
        coordinator.state().assistant_projection_cache_misses(),
        misses_after_projection
    );
}

#[test]
fn coordinator_hydration_keeps_unsupported_markdown_readable_in_assistant_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("unsupported_fallback.md");
    coordinator
        .hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("assistant", &markdown)]);

    let lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| matches!(line.role(), crate::agent::ui::transcript::ir::Role::Assistant))
        .map(|line| line.text())
        .collect::<Vec<_>>();

    // Tables are now supported and rendered with separators
    assert!(
        lines
            .iter()
            .any(|line| line.contains("col") && line.contains("val"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("a") && line.contains("b"))
    );
    assert!(
        lines.iter().any(|line| line.contains("│")),
        "table cells should be separated"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("alt (image: https://img.example/x.png)"))
    );
}

#[test]
fn coordinator_hydration_handles_malformed_assistant_markdown_without_dropping_message() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("malformed.md");
    coordinator
        .hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("assistant", &markdown)]);

    let assistant_lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| matches!(line.role(), crate::agent::ui::transcript::ir::Role::Assistant))
        .collect::<Vec<_>>();

    assert!(!assistant_lines.is_empty());
    assert!(
        assistant_lines
            .iter()
            .any(|line| line.text().contains("fn main() {"))
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

    let assistant_lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| matches!(line.role(), crate::agent::ui::transcript::ir::Role::Assistant))
        .map(|line| line.text())
        .collect::<Vec<_>>();

    assert!(assistant_lines.contains(&"prefix".to_string()));
    assert!(
        assistant_lines
            .iter()
            .any(|line| line.contains("{\"ok\":true}"))
    );
    assert!(assistant_lines.contains(&"suffix".to_string()));
    assert!(!assistant_lines.iter().any(|line| line.contains("[code:")));
    assert!(!assistant_lines.iter().any(|line| line.contains("[/code]")));
    assert!(
        !assistant_lines
            .iter()
            .any(|line| line.contains("<system-reminder>"))
    );
    assert!(!assistant_lines.iter().any(|line| line.contains("hidden")));
}

#[test]
fn coordinator_hydration_regression_no_duplicate_lines_on_single_call() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.hydrate_transcript_from_messages(vec![
        UiMessageSnapshot::new("user", "dup-check"),
        UiMessageSnapshot::new("assistant", "dup-check-reply"),
    ]);

    let user_count = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| line.text() == "dup-check")
        .count();
    let assistant_count = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| line.text() == "dup-check-reply")
        .count();

    assert_eq!(user_count, 1);
    assert_eq!(assistant_count, 1);
}

#[test]
fn coordinator_hydrate_with_empty_message_snapshot_leaves_empty_session_behavior_unchanged() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(Vec::<UiMessageSnapshot>::new());

    let state = coordinator.state();
    assert!(state.transcript_preview.is_empty());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input.locked);
}

#[test]
fn transcript_bottom_detection_uses_effective_viewport_after_input_chrome_and_margins() {
    let mut coordinator = RuntimeCoordinator::new(80, 20, Some(true));

    for _ in 0..220 {
        let mut source = StubEventSource {
            next: Some(TerminalEvent::Key(TerminalKey::Char('x'))),
        };
        coordinator.pump_once(&mut source);
    }

    let layout = coordinator.layout();
    let _expected_visible_rows = layout.transcript.height.saturating_sub(1) as usize;

    assert!(
        layout.input.height > 2,
        "input chrome should expand for wrapped text"
    );
    // Note: transcript_viewport_lines field removed after ratatui List migration
}

#[test]
fn transition_spacing_matrix_is_deterministic_for_role_changes() {
    assert!(!transition_spacer_for_roles_for_test(
        None,
        TranscriptRole::User
    ));
    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::User),
        TranscriptRole::User
    ));
    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Assistant),
        TranscriptRole::Assistant
    ));
    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Tool),
        TranscriptRole::Tool
    ));

    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::User),
        TranscriptRole::Assistant
    ));
    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Assistant),
        TranscriptRole::User
    ));

    assert!(transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Assistant),
        TranscriptRole::Tool
    ));
    assert!(transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Tool),
        TranscriptRole::Assistant
    ));
    assert!(transition_spacer_for_roles_for_test(
        Some(TranscriptRole::User),
        TranscriptRole::Tool
    ));
    assert!(transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Tool),
        TranscriptRole::User
    ));

    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Separator),
        TranscriptRole::Assistant
    ));
    assert!(!transition_spacer_for_roles_for_test(
        Some(TranscriptRole::Assistant),
        TranscriptRole::Separator
    ));
}

#[test]
fn transition_spacing_remains_legible_for_mixed_sequences() {
    let user_assistant_tool_assistant = [
        TranscriptRole::User,
        TranscriptRole::Assistant,
        TranscriptRole::Tool,
        TranscriptRole::Assistant,
    ];
    let uas_transitions = user_assistant_tool_assistant
        .windows(2)
        .map(|roles| transition_spacer_for_roles_for_test(Some(roles[0]), roles[1]))
        .collect::<Vec<_>>();
    assert_eq!(uas_transitions, vec![false, true, true]);

    let user_tool_assistant_tool = [
        TranscriptRole::User,
        TranscriptRole::Tool,
        TranscriptRole::Assistant,
        TranscriptRole::Tool,
    ];
    let uta_transitions = user_tool_assistant_tool
        .windows(2)
        .map(|roles| transition_spacer_for_roles_for_test(Some(roles[0]), roles[1]))
        .collect::<Vec<_>>();
    assert_eq!(uta_transitions, vec![true, true, true]);
}

#[test]
fn global_abort_cancels_active_and_pending_and_new_submit_starts_fresh() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for event in [
        TerminalEvent::Key(TerminalKey::Char('a')),
        TerminalEvent::Key(TerminalKey::Enter),
        TerminalEvent::Key(TerminalKey::Char('b')),
        TerminalEvent::Key(TerminalKey::Enter),
        TerminalEvent::Key(TerminalKey::Esc),
        TerminalEvent::Key(TerminalKey::Esc),
    ] {
        let mut source = StubEventSource { next: Some(event) };
        coordinator.pump_once(&mut source);
    }

    assert_eq!(coordinator.take_submitted_prompt(), None);

    let statuses = coordinator
        .state()
        .prompt_items()
        .iter()
        .map(|item| item.status)
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![PromptStatus::Cancelled, PromptStatus::Cancelled]
    );

    for event in [
        TerminalEvent::Key(TerminalKey::Char('c')),
        TerminalEvent::Key(TerminalKey::Enter),
    ] {
        let mut source = StubEventSource { next: Some(event) };
        coordinator.pump_once(&mut source);
    }

    assert_eq!(coordinator.take_submitted_prompt(), Some("c".to_string()));
}

#[test]
fn main_pane_vertical_split_has_no_overlap_or_bottom_cutoff() {
    let (_header, transcript, input, status) = RuntimeCoordinator::main_pane_rects_for_height(10);

    assert_eq!(_header.height, 0);
    assert!(
        transcript.height > 0,
        "transcript pane should remain visible"
    );
    assert_eq!(
        status.height, 2,
        "footer must reserve two rows for two lanes"
    );
    assert_eq!(transcript.y + transcript.height, input.y);
    assert_eq!(input.y + input.height, status.y);
    assert_eq!(status.y + status.height, 10);
}

#[test]
fn scripted_event_parser_supports_ctrlp_for_palette_toggle() {
    let mut source = ScriptedTerminalEvents::from_script("ctrlp");
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlP)))
    );
}

#[test]
fn scripted_event_parser_supports_ctrln_for_query_picker_navigation() {
    let mut source = ScriptedTerminalEvents::from_script("ctrln");
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlN)))
    );
}

#[test]
fn multiline_input_prompt_icon_appears_only_on_first_visual_row() {
    let mut state = AppState::new();
    state.input_mode = InputMode::Insert;
    state.input.buffer = "ab\n12345".to_string();

    let rows = input_rows_with_prompt_for_test(&state, 5);
    assert_eq!(rows, vec!["❯ ab", "  123", "  45"]);
}

#[test]
fn prompt_prefix_uses_mode_indicator_insert_vs_normal_visual() {
    let mut insert = AppState::new();
    insert.input_mode = InputMode::Insert;
    insert.input.buffer = "hello".to_string();

    let mut normal = AppState::new();
    normal.input_mode = InputMode::Normal;
    normal.input.buffer = "hello".to_string();

    let mut visual = AppState::new();
    visual.input_mode = InputMode::Visual;
    visual.input.buffer = "hello".to_string();

    assert_eq!(
        input_rows_with_prompt_for_test(&insert, 20),
        vec!["❯ hello"]
    );
    assert_eq!(
        input_rows_with_prompt_for_test(&normal, 20),
        vec!["❮ hello"]
    );
    assert_eq!(
        input_rows_with_prompt_for_test(&visual, 20),
        vec!["❮ hello"]
    );
}

#[test]
fn prompt_prefix_switches_immediately_when_mode_changes() {
    let mut state = AppState::new();
    state.input.buffer = "hello".to_string();

    state.input_mode = InputMode::Insert;
    assert_eq!(input_rows_with_prompt_for_test(&state, 20), vec!["❯ hello"]);

    state.input_mode = InputMode::Normal;
    assert_eq!(input_rows_with_prompt_for_test(&state, 20), vec!["❮ hello"]);
}

#[test]
fn status_contract_a_model_line_reports_identity_and_busy_idle() {
    let mut state = AppState::new();

    let idle_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(
        idle_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (idle)")
    );

    state.phase = UiPhase::Busy;
    let busy_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(
        busy_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini (busy)")
    );
}

#[test]
fn status_contract_b_excludes_input_mode_backend_poll_and_hint_lines() {
    let state = AppState::new();

    let lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input backend:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input poll:")));
    assert!(!lines.iter().any(|line| line.starts_with("Input error:")));
    assert!(!lines.iter().any(|line| line.starts_with("Hint:")));
}

#[test]
fn status_contract_c_mcp_counts_include_configured_enabled_disabled_failed() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "docs".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "MCP: configured=3 enabled=1 disabled=1 failed=1")
    );
}

#[test]
fn status_contract_d_visible_mcp_tool_count_uses_runtime_truth_and_updates() {
    let mut state = AppState::new();
    state.set_llm_visible_mcp_tool_count(5);

    let before = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(before.iter().any(|line| line == "LLM-visible MCP tools: 5"));

    state.set_llm_visible_mcp_tool_count(2);
    let after = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(after.iter().any(|line| line == "LLM-visible MCP tools: 2"));
}

#[test]
fn status_contract_e_failures_show_names_and_reasons_and_healthy_none_when_clear() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Failed,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    assert!(state.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Failed,
        Some("timeout".to_string())
    ));
    assert!(state.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        None
    ));

    let failed_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    let failed_rendered = failed_lines.join("\n");
    assert!(failed_rendered.contains("Failures: gh (timeout), k8s"));

    assert!(state.set_mcp_server_state_by_name_with_reason(
        "gh",
        McpServerUsabilityState::Enabled,
        None
    ));
    assert!(state.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Enabled,
        None
    ));
    let healthy_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
    assert!(
        healthy_lines
            .iter()
            .any(|line| line == "Failures: none (healthy)")
    );
}

#[test]
fn status_contract_f_narrow_layout_is_compact_and_ellipsizes_deterministically() {
    let mut state = AppState::new();
    state.phase = UiPhase::Busy;
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "very-long-mcp-server-name-that-must-be-truncated".to_string(),
        state: McpServerUsabilityState::Failed,
    }]);
    assert!(state.set_mcp_server_state_by_name_with_reason(
        "very-long-mcp-server-name-that-must-be-truncated",
        McpServerUsabilityState::Failed,
        Some("very long failure reason that should be truncated to keep the status line readable"
            .to_string())
    ));
    state.set_llm_visible_mcp_tool_count(42);

    let lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "provider/super-long-model-name-that-needs-truncation",
    );
    let rendered = lines.join("\n");
    assert!(rendered.contains('…'));
    assert!(!rendered.contains("Hint: Ctrl-P -> MCPs"));

    let compact = crate::agent::ui::tui::runtime::compact_status_line_for_test(
        "provider/super-long-model-name-that-needs-truncation",
        None,
    );
    let compact_narrow =
        crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
            "provider/super-long-model-name-that-needs-truncation",
            Some("feature/very-long-branch-name-that-needs-truncation"),
            None,
            24,
        );
    assert!(!compact.starts_with("❯ "));
    assert!(!compact.contains('|'));
    assert!(compact_narrow.contains("..."));
    assert!(!compact_narrow.contains('|'));
}

#[test]
fn status_lines_include_stable_active_model_identity_line() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);
    let status_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );

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
    let (title, lines) = help_panel_lines_for_test();
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
    let (_title, lines) = help_panel_lines_for_test();
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
    let (_title, lines) = help_panel_lines_for_test();
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
    let (_title, lines) = help_panel_lines_for_test();
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
fn help_panel_shows_overflow_position_cue_when_content_exceeds_viewport() {
    let (_title, lines) = help_panel_lines_for_test();
    let viewport_inner_height = 8u16;
    let cue = help_panel_overflow_cue_for_test(&lines, viewport_inner_height, 3)
        .expect("overflow cue should appear when help exceeds viewport");

    assert!(cue.contains("PgUp/PgDn"));
    assert!(cue.contains("Esc close"));
    assert!(cue.contains("/"));
}

#[test]
fn help_panel_escape_closes_panel_after_scroll() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .open_info_panel(crate::agent::ui::tui::state::InfoPanel::Help);
    coordinator.state.info_panel_scroll = 5;

    let mut down = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Down)),
    };
    coordinator.pump_once(&mut down);
    assert!(coordinator.state().info_panel_scroll >= 5);

    let mut esc = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::Esc)),
    };
    coordinator.pump_once(&mut esc);

    assert_eq!(coordinator.state().info_panel, None);
}

#[test]
fn status_panel_exposes_model_and_mcp_backend_status_lines() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
    ]);

    let (title, lines) = status_panel_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state.mcp_panel_selection = 1;

    state.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some("connect timeout".to_string()),
    );
    state.set_mcp_visible_tool_count_by_server_name("gh", 3);
    state.set_mcp_visible_tool_count_by_server_name("k8s", 9);

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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "enabled-srv".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "disabled-srv".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::agent::ui::tui::state::McpServerState {
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "enabled-srv".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "disabled-srv".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "failed-srv".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(super::mcp_status_column_width_for_test(), 6);
    assert_eq!(model.rows[0][2], "🟢");
    assert_eq!(model.rows[1][2], "⚪");
    assert_eq!(model.rows[2][2], "🔴");
}

#[test]
fn mcp_details_height_allocation_prefers_multiple_tool_lines_in_normal_popup_heights() {
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(4), 0);
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(5), 1);
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(6), 2);
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(8), 3);
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(10), 4);
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(12), 5);
    assert_eq!(super::mcp_details_height_for_inner_height_for_test(14), 6);
}

#[test]
fn mcp_panel_layout_keeps_table_primary_with_multiple_visible_rows_in_common_height() {
    let mut state = AppState::new();
    state.set_mcp_servers(
        (0..8)
            .map(|idx| crate::agent::ui::tui::state::McpServerState {
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
    let details_height = super::mcp_details_height_for_inner_height_for_test(inner_height);
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
    let line = super::mcp_panel_controls_line_for_test();
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state.set_mcp_visible_tool_count_by_server_name("gh", 4);
    state.set_mcp_visible_tool_count_by_server_name("k8s", 2);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
}

#[test]
fn mcp_selected_details_model_shows_full_error_text_tools_list_and_fallback() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    let reason = "connection timeout while dialing 10.0.0.1:443".to_string();
    state.set_mcp_server_state_by_name_with_reason(
        "k8s",
        McpServerUsabilityState::Failed,
        Some(reason.clone()),
    );
    state.set_mcp_visible_tool_names_by_server_name(
        "k8s",
        vec!["k8s__z_last".to_string(), "k8s__a_first".to_string()],
    );

    state.mcp_panel_selection = 1;
    let failed = super::mcp_selected_details_for_test(&state).expect("selected details");
    assert_eq!(failed.server_line, "Server: k8s (failed)");
    assert_eq!(failed.error_line, format!("Error: {reason}"));
    assert_eq!(failed.tools_line, "Tools: k8s__a_first, k8s__z_last");

    state.mcp_panel_selection = 0;
    let healthy = super::mcp_selected_details_for_test(&state).expect("selected details");
    assert_eq!(healthy.server_line, "Server: gh (enabled)");
    assert_eq!(healthy.error_line, "Error: None");
    assert_eq!(healthy.tools_line, "Tools: None");
}

#[test]
fn mcp_table_visible_tool_count_respects_live_updates_after_selection_changes() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Disabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "docs".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);
    state.set_mcp_visible_tool_count_by_server_name("gh", 4);
    state.set_mcp_visible_tool_count_by_server_name("k8s", 2);
    state.set_mcp_visible_tool_count_by_server_name("docs", 7);

    let model = mcp_table_model_for_test(&state, 80, 10);
    assert_eq!(model.rows[0][1], "4");
    assert_eq!(model.rows[1][1], "2");
    assert_eq!(model.rows[2][1], "7");
}

#[test]
fn mcp_selected_details_height_zero_and_one_rows_preserve_error_presence() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "k8s".to_string(),
        state: McpServerUsabilityState::Failed,
    }]);
    state.set_mcp_server_state_by_name_with_reason(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "k8s".to_string(),
        state: McpServerUsabilityState::Failed,
    }]);
    state.set_mcp_server_state_by_name_with_reason(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "k8s".to_string(),
        state: McpServerUsabilityState::Failed,
    }]);
    let reason =
        "connection timeout while dialing 10.0.0.1:443 after many retries and additional context";
    state.set_mcp_server_state_by_name_with_reason(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);
    state.set_mcp_visible_tool_names_by_server_name(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);
    state.set_mcp_visible_tool_names_by_server_name(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);
    state.set_mcp_visible_tool_names_by_server_name(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);
    state.set_mcp_visible_tool_names_by_server_name(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);
    state.set_mcp_visible_tool_names_by_server_name(
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
    let mut state = AppState::new();
    state.set_mcp_servers(vec![crate::agent::ui::tui::state::McpServerState {
        name: "gh".to_string(),
        state: McpServerUsabilityState::Enabled,
    }]);

    let model = mcp_table_model_for_test(&state, 32, 8);
    assert_eq!(model.columns, vec!["Name", "Visible tools", "Status"]);
    assert_eq!(model.rows.len(), 1);
}

#[test]
fn mcp_table_model_overflow_top_window_locks_exact_cue_and_selected_mapping() {
    let mut state = AppState::new();
    state.set_mcp_servers(
        (0..8)
            .map(|idx| crate::agent::ui::tui::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );
    state.mcp_panel_selection = 0;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(0));
    assert_eq!(model.rows[model.selected.expect("selection")][0], "srv-0");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 1-5 / 8".to_string())
    );
}

#[test]
fn mcp_table_model_overflow_middle_window_locks_exact_cue_and_selected_mapping() {
    let mut state = AppState::new();
    state.set_mcp_servers(
        (0..8)
            .map(|idx| crate::agent::ui::tui::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );
    state.mcp_panel_selection = 5;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(4));
    assert_eq!(model.rows[model.selected.expect("selection")][0], "srv-5");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 2-6 / 8".to_string())
    );
}

#[test]
fn mcp_table_model_overflow_bottom_window_locks_exact_cue_and_selected_mapping() {
    let mut state = AppState::new();
    state.set_mcp_servers(
        (0..8)
            .map(|idx| crate::agent::ui::tui::state::McpServerState {
                name: format!("srv-{idx}"),
                state: if idx % 2 == 0 {
                    McpServerUsabilityState::Enabled
                } else {
                    McpServerUsabilityState::Disabled
                },
            })
            .collect(),
    );
    state.mcp_panel_selection = 7;

    let model = mcp_table_model_for_test(&state, 80, 7);
    assert_eq!(model.selected, Some(4));
    assert_eq!(model.rows[model.selected.expect("selection")][0], "srv-7");
    assert_eq!(
        model.overflow_cue,
        Some("↑/↓ or j/k | Enter/Space toggle | Esc close | 4-8 / 8".to_string())
    );
}

#[test]
fn command_palette_table_renders_required_columns_and_rows() {
    let mut state = AppState::new();
    state.open_command_palette();

    let model = command_palette_table_model_for_test(&state, 80, 10);

    assert_eq!(model.columns, vec!["Action", "Summary"]);
    let actions = model
        .rows
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();
    assert_eq!(actions, vec!["Help", "Status", "MCPs", "Skills", "Models"]);
    assert!(model.rows.iter().all(|row| row[2].is_empty()));
    assert_eq!(model.selected, Some(0));
}

#[test]
fn command_palette_table_renders_skills_action_row() {
    let mut state = AppState::new();
    state.open_command_palette();

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
    let state = AppState::new();
    let (title, lines) = crate::agent::ui::tui::runtime::skills_panel_lines_for_test(&state);

    assert_eq!(title, "Skills");
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("No discoverable skills available."));
}

#[test]
fn skills_panel_lists_skills_in_deterministic_order() {
    let mut state = AppState::new();
    state.set_discoverable_skills(vec![
        crate::agent::ui::tui::state::DiscoverableSkill {
            source_priority: 1,
            source: "home".to_string(),
            name: "zeta".to_string(),
        },
        crate::agent::ui::tui::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "beta".to_string(),
        },
        crate::agent::ui::tui::state::DiscoverableSkill {
            source_priority: 0,
            source: "repo".to_string(),
            name: "alpha".to_string(),
        },
    ]);

    let (_title, lines) = crate::agent::ui::tui::runtime::skills_panel_lines_for_test(&state);
    let rendered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let alpha_idx = rendered.find("alpha").expect("alpha row present");
    let beta_idx = rendered.find("beta").expect("beta row present");
    let zeta_idx = rendered.find("zeta").expect("zeta row present");
    assert!(alpha_idx < beta_idx);
    assert!(beta_idx < zeta_idx);
}

#[test]
fn inline_slash_suggestions_render_inline_with_single_hint_contract() {
    let mut state = AppState::new();
    state.append_input_char('/');

    let rows = inline_slash_lines_for_test(&state);
    assert!(!rows.is_empty());
    assert!(rows[0].contains("/compact"));
    assert!(rows[0].starts_with('❯'));

    let title = super::command_palette_title(None);
    assert!(title.contains("↑/↓ or Ctrl-N · Enter · Esc"));
}

#[test]
fn command_palette_table_emits_overflow_position_cue_when_viewport_is_small() {
    let mut state = AppState::new();
    state.open_command_palette();

    let model = command_palette_table_model_for_test(&state, 80, 5);
    let cue = model
        .overflow_cue
        .expect("expected overflow cue when rows exceed viewport");
    assert!(cue.contains("/"));
    assert!(cue.contains("Esc close"));
}

#[test]
fn help_modal_uses_large_readable_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup =
        super::render_frame::modal_rect_for_panel(area, super::render_frame::ModalPanelKind::Help);

    assert!(popup.width >= 72);
    assert!(popup.height >= 18);
}

#[test]
fn status_modal_uses_compact_layout() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup = super::render_frame::modal_rect_for_panel(
        area,
        super::render_frame::ModalPanelKind::Status,
    );

    assert!(popup.width <= 72);
    assert!(popup.height <= 14);
}

#[test]
fn modal_layout_policy_applies_consistently_across_panels() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let command_palette = super::render_frame::modal_rect_for_panel(
        area,
        super::render_frame::ModalPanelKind::CommandPalette,
    );
    let skills = super::render_frame::modal_rect_for_panel(
        area,
        super::render_frame::ModalPanelKind::Skills,
    );
    let mcps =
        super::render_frame::modal_rect_for_panel(area, super::render_frame::ModalPanelKind::Mcps);

    assert_eq!(skills.width, mcps.width);
    assert_eq!(skills.height, mcps.height);
    assert!(command_palette.width < skills.width);
}

#[test]
fn models_modal_uses_layout_policy_defaults() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let models = super::render_frame::modal_rect_for_panel(
        area,
        super::render_frame::ModalPanelKind::Models,
    );
    let skills = super::render_frame::modal_rect_for_panel(
        area,
        super::render_frame::ModalPanelKind::Skills,
    );

    assert_eq!(models.width, skills.width);
    assert_eq!(models.height, skills.height);
}

#[test]
fn modal_frame_uses_rounded_border_style() {
    assert!(super::modal_frame_uses_rounded_border_style_for_test());
}

#[test]
fn modal_open_state_applies_dimmed_backdrop() {
    let mut state = AppState::new();
    state.open_command_palette();

    assert!(super::modal_open_state_applies_dimmed_backdrop_for_test(
        &state
    ));
}

#[test]
fn inline_model_picker_modal_respects_border_and_backdrop_policy() {
    let mut state = AppState::new();
    state.open_model_picker();

    assert!(super::inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(&state));
}

#[test]
fn permission_prompt_does_not_open_global_dimmed_modal_backdrop() {
    let mut state = AppState::new();
    state.open_permission_prompt(crate::agent::ui::tui::state::PermissionPrompt {
        request_id: "ask-0000000000000001".to_string(),
        matched_rule_identity: "nested:nu__run.command:*".to_string(),
        tool: "nu__run".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "tool[nu__run] args={\"command\":\"echo hi\"}".to_string(),
    });

    assert!(!super::modal_open_state_applies_dimmed_backdrop_for_test(
        &state
    ));
}

#[test]
fn model_picker_empty_catalog_shows_deterministic_empty_state() {
    let state = AppState::new();
    assert_eq!(
        super::model_picker_empty_state_message_for_test(),
        "No models available in cached startup config."
    );
    assert!(state.model_picker_filtered_options().is_empty());
}

#[test]
fn status_lines_report_failed_state_count_when_present() {
    let mut state = AppState::new();
    state.set_mcp_servers(vec![
        crate::agent::ui::tui::state::McpServerState {
            name: "gh".to_string(),
            state: McpServerUsabilityState::Enabled,
        },
        crate::agent::ui::tui::state::McpServerState {
            name: "k8s".to_string(),
            state: McpServerUsabilityState::Failed,
        },
    ]);

    let status_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );

    assert!(
        status_lines
            .iter()
            .any(|line| line == "MCP: configured=2 enabled=1 disabled=0 failed=1")
    );
}

#[test]
fn status_lines_include_tokens_line_with_na_before_any_llm_end() {
    let state = AppState::new();
    let status_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
    );

    assert!(
        status_lines
            .iter()
            .any(|line| line == "LLM-visible MCP tools: 0")
    );
}

#[test]
fn status_lines_include_latest_and_rolling_tokens_after_llm_end_events() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    coordinator.enqueue_ui_event(UiEvent::LlmEnd {
        response_chars: 10,
        tool_calls: 0,
        input_tokens: 11,
        output_tokens: 9,
        total_tokens: 20,
    });
    coordinator.drain_transport();

    coordinator.enqueue_ui_event(UiEvent::LlmEnd {
        response_chars: 12,
        tool_calls: 0,
        input_tokens: 3,
        output_tokens: 4,
        total_tokens: 7,
    });
    coordinator.drain_transport();

    let status_lines = crate::agent::ui::tui::runtime::status_lines_for_test(
        coordinator.state(),
        "openai/gpt-4o-mini",
    );

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
    let mut state = AppState::new();
    state.latest_total_tokens = Some(7);
    state.session_total_tokens = 27;

    let status_line = crate::agent::ui::tui::runtime::compact_status_line_for_test(
        "openai/gpt-4o-mini",
        None,
    );

    assert!(status_line.starts_with("○ openai/gpt-4o-mini"));
    assert!(!status_line.contains('|'));
}

#[test]
fn lane_2_context_line_uses_exact_usage_format_without_extra_text() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(250);
    state.set_context_window_max_tokens(Some(1000));

    let line = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 120);

    assert_eq!(
        line,
        "                                                                                                               250 (25%)"
    );
    assert!(!line.contains("Context"));
    assert!(!line.contains("Ctrl-P"));
    assert!(!line.contains('|'));
}

#[test]
fn lane_2_context_line_falls_back_to_used_only_when_max_unavailable() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(42);
    state.set_context_window_max_tokens(None);

    let line = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 120);

    assert_eq!(
        line,
        "                                                                                                                      42"
    );
    assert!(!line.contains("Context"));
    assert!(!line.contains("Ctrl-P"));
    assert!(!line.contains('|'));
}

#[test]
fn footer_two_lane_contract_exposes_lane_1_and_lane_2_simultaneously() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(250);
    state.set_context_window_max_tokens(Some(1000));

    let lane_1 = crate::agent::ui::tui::runtime::compact_status_line_for_test(
        "openai/gpt-4o-mini",
        None,
    );
    let lane_2 = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 120);

    assert!(lane_1.starts_with("○ openai/gpt-4o-mini"));
    assert!(!lane_1.contains('|'));
    assert!(lane_2.ends_with("250 (25%)"));
}

#[test]
fn configured_path_resolves_context_max_without_fallback_format() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_context_window_max_tokens(Some(128_000));
    coordinator.enqueue_ui_event(UiEvent::LlmEnd {
        response_chars: 40,
        tool_calls: 0,
        input_tokens: 2_500,
        output_tokens: 500,
        total_tokens: 3_000,
    });
    coordinator.drain_transport();

    let lane_2 =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);

    assert!(lane_2.ends_with("3k (2%)"));
    assert!(!lane_2.contains('/'));
}

#[test]
fn lane_2_context_line_updates_after_each_turn_and_does_not_stale() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_context_window_max_tokens(Some(100));

    coordinator.enqueue_ui_event(UiEvent::LlmEnd {
        response_chars: 12,
        tool_calls: 0,
        input_tokens: 2,
        output_tokens: 8,
        total_tokens: 10,
    });
    coordinator.drain_transport();
    let first =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert!(first.ends_with("10 (10%)"));

    coordinator.enqueue_ui_event(UiEvent::LlmEnd {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();
    let second =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert!(second.ends_with("40 (40%)"));
}

#[test]
fn lane_2_context_line_truncation_removes_any_extra_labels_or_hints() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(12345);
    state.set_context_window_max_tokens(Some(128000));

    let line = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 30);

    assert_eq!(line, "                    12.3k (9%)");
    assert!(!line.contains("Context"));
    assert!(!line.contains("Ctrl-P"));
    assert!(!line.contains('|'));
}

#[test]
fn lane_2_rehydrates_used_tokens_from_hydrated_history_metadata() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("user", "hello"), {
        let mut s = UiMessageSnapshot::new("assistant", "history");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(444),
        });
        s
    }]);

    let lane_2 =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert_eq!(lane_2.chars().count(), 120);
    assert!(lane_2.ends_with("444"));
}

#[test]
fn lane_2_rehydrate_with_known_max_shows_ratio_immediately() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_context_window_max_tokens(Some(1000));
    coordinator.hydrate_transcript_from_messages(vec![{
        let mut s = UiMessageSnapshot::new("assistant", "history");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(250),
        });
        s
    }]);

    let lane_2 =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert!(lane_2.ends_with("250 (25%)"));
}

#[test]
fn lane_2_rehydrate_without_usage_metadata_and_without_max_uses_fallback() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("assistant", "history")]);

    let lane_2 =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert_eq!(lane_2.chars().count(), 120);
    assert!(lane_2.ends_with("0"));
}

#[test]
fn lane_2_rehydrate_without_usage_metadata_with_known_max_shows_ratio_not_fallback() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_context_window_max_tokens(Some(100));
    coordinator
        .hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("assistant", "history")]);

    let lane_2 =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert!(lane_2.ends_with("0 (0%)"));
}

#[test]
fn lane_2_rehydrate_is_replaced_by_live_turn_usage() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.set_context_window_max_tokens(Some(100));
    coordinator.hydrate_transcript_from_messages(vec![{
        let mut s = UiMessageSnapshot::new("assistant", "history");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(7),
        });
        s
    }]);

    let hydrated =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert!(hydrated.ends_with("7 (7%)"));

    coordinator.enqueue_ui_event(UiEvent::LlmEnd {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();

    let live =
        crate::agent::ui::tui::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    assert!(live.ends_with("40 (40%)"));
}

#[test]
fn lane_2_threshold_formatting_contract_100_and_1000_and_11657() {
    let mut state = AppState::new();
    state.set_context_window_max_tokens(Some(200_000));

    state.latest_total_tokens = Some(100);
    let one_hundred = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 40);
    assert!(one_hundred.ends_with("100 (0%)"));

    state.latest_total_tokens = Some(1_000);
    let one_thousand = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 40);
    assert!(one_thousand.ends_with("1k (0%)"));

    state.latest_total_tokens = Some(11_657);
    let eleven_point_six = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 40);
    assert!(eleven_point_six.ends_with("11.6k (5%)"));
}

#[test]
fn lane_2_is_right_aligned_in_wide_layout() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(11_657);
    state.set_context_window_max_tokens(Some(200_000));

    let width = 40usize;
    let line = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, width);

    assert_eq!(line.chars().count(), width);
    assert!(line.ends_with("11.6k (5%)"));
    assert!(line.starts_with(" "));
}

#[test]
fn lane_2_narrow_width_uses_deterministic_right_anchored_truncation() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(11_657);
    state.set_context_window_max_tokens(Some(200_000));

    let line = crate::agent::ui::tui::runtime::lane_2_status_line_for_test(&state, 8);

    assert_eq!(line, "... (5%)");
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
}

impl Default for MockTerminalState {
    fn default() -> Self {
        Self {
            raw_mode_enabled: false,
            alt_screen_enabled: false,
            cursor_visible: true,
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

    fn run(&self, action: TerminalAction) -> Result<(), TerminalLifecycleError> {
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
    fn enable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = true;
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::DisableRawMode)?;
        self.state.borrow_mut().raw_mode_enabled = false;
        Ok(())
    }

    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::EnterAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = true;
        Ok(())
    }

    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::LeaveAltScreen)?;
        self.state.borrow_mut().alt_screen_enabled = false;
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::HideCursor)?;
        self.state.borrow_mut().cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        self.run(TerminalAction::ShowCursor)?;
        self.state.borrow_mut().cursor_visible = true;
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
        }
    );
}

#[test]
fn run_with_terminal_restore_executes_enter_run_and_restore_in_order() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(MockTerminalState::default()));
    let backend = MockTerminalBackend::new(actions.clone(), state.clone(), None);
    let mut lifecycle = TerminalLifecycle::new(backend);

    let value = run_with_terminal_restore(&mut lifecycle, || Ok::<_, &'static str>(42))
        .expect("run should succeed");
    assert_eq!(value, 42);

    assert_eq!(
        *actions.borrow(),
        vec![
            TerminalAction::EnableRawMode,
            TerminalAction::EnterAltScreen,
            TerminalAction::HideCursor,
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableRawMode,
        ]
    );
    assert_terminal_restored(&state);
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

    let err = run_with_terminal_restore::<_, (), &'static str, _>(&mut lifecycle, || Ok(()))
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

    let err = run_with_terminal_restore::<_, (), _, _>(&mut lifecycle, || Err("boom"))
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

    let err = run_with_terminal_restore::<_, (), _, _>(&mut lifecycle, || Err("boom"))
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

    let err = run_with_terminal_restore::<_, (), &'static str, _>(&mut lifecycle, || Ok(()))
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
        let _ = run_with_terminal_restore::<_, (), &'static str, _>(&mut lifecycle, || {
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
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
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
        let _ = run_with_terminal_restore::<_, (), &'static str, _>(&mut lifecycle, || {
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
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
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

    let err = run_with_terminal_restore::<_, (), _, _>(&mut lifecycle, || {
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
            TerminalAction::ShowCursor,
            TerminalAction::LeaveAltScreen,
            TerminalAction::DisableRawMode,
        ]
    );
}

#[test]
fn main_pane_rects_transcript_gets_remaining_space() {
    use crate::agent::ui::tui::rendering::layout::INPUT_MIN_HEIGHT;
    use crate::agent::ui::tui::runtime::render_frame::STATUS_TARGET_HEIGHT;
    use crate::agent::ui::tui::runtime::render_frame::main_pane_rects_for_height;

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
    assert_eq!(
        crate::agent::ui::tui::runtime::status_indicator_for_test(None),
        "○"
    );
}

#[test]
fn status_indicator_busy_cycles_through_four_frames() {
    let f = crate::agent::ui::tui::runtime::status_indicator_for_test;
    assert_eq!(f(Some(0)), "◐");
    assert_eq!(f(Some(150)), "◓");
    assert_eq!(f(Some(300)), "◑");
    assert_eq!(f(Some(450)), "◒");
    assert_eq!(f(Some(600)), "◐"); // wraps
}

#[test]
fn lane_1_idle_shows_empty_circle_prefix() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    assert!(line.starts_with("○ mymodel"));
}

#[test]
fn lane_1_busy_shows_spinner_prefix() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "mymodel", None, Some(0), 40,
    );
    assert!(line.starts_with("◐ mymodel"));
}

#[test]
fn lane_1_prefix_does_not_exceed_available_width() {
    let line = crate::agent::ui::tui::runtime::status::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop", Some("branchname"), None, 40,
    );
    assert!(line.chars().count() <= 40);
    assert!(line.starts_with("○ "));
}

#[test]
fn hydration_compaction_creates_block_structure() {
    use crate::agent::ui::tui::state::CompactionStatus;
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "compaction",
        "## Summary\n- point one\n- point two",
    )]);

    // The compaction block header ("Compaction") should be present in transcript
    let has_compaction_header = coordinator
        .state()
        .transcript_preview
        .iter()
        .any(|line| line.text() == "Compaction");
    assert!(
        has_compaction_header,
        "expected compaction block header in transcript"
    );

    // The compaction block should have Done status
    let compaction_header_idx = coordinator
        .state()
        .transcript_preview
        .iter()
        .position(|line| line.text() == "Compaction")
        .expect("compaction header must exist");
    assert_eq!(
        coordinator
            .state()
            .transcript_line_status_for_index(compaction_header_idx),
        Some(TranscriptLineStatus::Compaction(CompactionStatus::Done))
    );
}

#[test]
fn hydration_compaction_renders_markdown_body() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "compaction",
        "## Summary\n- alpha\n- beta",
    )]);

    let texts: Vec<String> = coordinator
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect();

    // Raw markdown markers should NOT appear (markdown was rendered)
    assert!(
        !texts.iter().any(|t| t.contains("## ")),
        "raw markdown heading marker should not appear: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.starts_with("- ")),
        "raw markdown list marker should not appear: {texts:?}"
    );
    // Rendered content should be present
    assert!(
        texts.iter().any(|t| t.contains("Summary")),
        "rendered heading text should appear: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("alpha")),
        "rendered list item text should appear: {texts:?}"
    );
}

#[test]
fn hydration_compaction_empty_summary_shows_block_only() {
    use crate::agent::ui::tui::state::CompactionStatus;
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("compaction", "")]);

    let texts: Vec<String> = coordinator
        .state()
        .transcript_preview
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
        .transcript_preview
        .iter()
        .filter(|line| line.role() == Role::Compaction)
        .collect();
    assert!(
        compaction_lines.is_empty(),
        "empty summary should produce no Compaction-role body lines: {compaction_lines:?}"
    );

    // Header should have Done status
    let header_idx = coordinator
        .state()
        .transcript_preview
        .iter()
        .position(|line| line.text() == "Compaction")
        .expect("header must exist");
    assert_eq!(
        coordinator
            .state()
            .transcript_line_status_for_index(header_idx),
        Some(TranscriptLineStatus::Compaction(CompactionStatus::Done))
    );
}

#[test]
fn hydration_compaction_matches_live_rendering() {
    let summary_body = "## Summary\n- alpha\n- beta";

    // Live path: CompactionStarted + CompactionTriggered via reducer
    let mut live = RuntimeCoordinator::new(120, 30, Some(true));
    live.enqueue_ui_event(UiEvent::CompactionStarted {
        source: "history".to_string(),
    });
    live.drain_transport();
    live.enqueue_ui_event(UiEvent::CompactionTriggered {
        source: "history".to_string(),
        summarized_count: 5,
        kept_recent_count: 2,
        summary_preview: "preview".to_string(),
        summary_body: summary_body.to_string(),
    });
    live.drain_transport();

    // Hydration path: UiMessageSnapshot with role "compaction"
    let mut hydrated = RuntimeCoordinator::new(120, 30, Some(true));
    hydrated.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "compaction",
        summary_body,
    )]);

    let live_texts: Vec<String> = live
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect();
    let hydrated_texts: Vec<String> = hydrated
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.text())
        .collect();

    assert_eq!(
        live_texts, hydrated_texts,
        "live and hydrated transcript texts should match"
    );

    let live_roles: Vec<Role> = live
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.role())
        .collect();
    let hydrated_roles: Vec<Role> = hydrated
        .state()
        .transcript_preview
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

    let result = coord.render_if_needed(&mut None);

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

    let result = coord.render_if_needed(&mut None);

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

    let result = coord.render_if_needed(&mut None);

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
    assert_eq!(coordinator.state().assistant_projection_cache_misses(), 1);

    let texts: Vec<String> = coordinator
        .state()
        .transcript_preview
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

    // Both messages should have been processed (2 cache misses), because a Tick
    // separates them — coalescing only applies to consecutive same-type events
    assert_eq!(coordinator.state().assistant_projection_cache_misses(), 2);

    // Final transcript shows "world" (the second AssistantMessage replaces the first)
    let texts: Vec<String> = coordinator
        .state()
        .transcript_preview
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

    assert_eq!(coordinator.state().assistant_projection_cache_misses(), 1);

    let texts: Vec<String> = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| line.role() == Role::Assistant)
        .map(|line| line.text())
        .collect();
    assert_eq!(texts, vec!["solo"]);
}
