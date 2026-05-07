use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

use ratatui::style::{Color, Modifier};

use crate::commands::agent::contracts::UiMessageSnapshot;
use crate::commands::agent::ui::{
    event::UiEvent,
    renderer::UiRenderer,
    tui::{
        input::{TerminalEvent, TerminalKey},
        runtime::{
            InputSourceDiagnostics, RuntimeCoordinator, RuntimeRunError, ScriptedTerminalEvents,
            TerminalEventSource, TuiRuntimeRenderer, cursor_style_for_test, input_line_for_test,
            input_rows_with_prompt_for_test,
            input_line_for_test_at_millis, prompt_indicator_for_status_for_test,
            render_transcript_lines_for_test, run_with_terminal_restore,
            render_transcript_lines_with_flags_for_test,
            lane_prefix_spans_for_test, row_spans_for_test,
            indicator_style_for_status_for_test, transition_spacer_for_roles_for_test,
            parse_persisted_tool_status_line_for_test,
            transcript_title_for_test, visible_transcript_window,
            visible_transcript_window_for_render_for_test,
            visual_indicator_line_for_test,
        },
        safety::RestoreRunError,
        state::{
            AppState, InputMode, PaneFocus, PromptStatus, ToolCallStatus, TranscriptLineStatus,
            TranscriptRole, UiPhase,
        },
        terminal::{TerminalAction, TerminalBackend, TerminalLifecycle, TerminalLifecycleError},
    },
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crate::session::{Message, SessionStore};

const CTP_MOCHA_RED: Color = Color::Rgb(243, 139, 168);
const CTP_MOCHA_YELLOW: Color = Color::Rgb(249, 226, 175);
const CTP_MOCHA_GREEN: Color = Color::Rgb(166, 227, 161);
const CTP_MOCHA_BLUE: Color = Color::Rgb(137, 180, 250);
const CTP_MOCHA_SAPPHIRE: Color = Color::Rgb(116, 199, 236);
const CTP_MOCHA_OVERLAY0: Color = Color::Rgb(108, 112, 134);
const CTP_MOCHA_OVERLAY1: Color = Color::Rgb(127, 132, 156);
const CTP_MOCHA_SURFACE0: Color = Color::Rgb(49, 50, 68);
const CTP_MOCHA_SURFACE1: Color = Color::Rgb(69, 71, 90);

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

fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/commands/agent/ui/tui/fixtures/markdown")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read markdown fixture {}: {error}", path.display())
    })
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
    assert_eq!(
        coordinator.state().transcript_preview[0].role,
        TranscriptRole::User
    );
    assert_eq!(coordinator.state().transcript_preview[0].text, "x");
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
            crate::commands::agent::ui::tui::input::TerminalResize {
                columns: 160,
                rows: 40,
            },
        )),
    };
    coordinator.pump_once(&mut source);

    let after = coordinator.layout();
    assert_ne!(before, after);
    assert!(after.side_pane.is_none());
    assert_eq!(after.transcript.width, 160);
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
            crate::commands::agent::ui::tui::input::TerminalResize {
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
    assert_eq!(layout.transcript.width, 140);
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
        coordinator.state().transcript_preview[0].role,
        TranscriptRole::Assistant
    );
    assert_eq!(coordinator.state().transcript_preview[0].text, "hello");
    assert_eq!(
        coordinator.state().transcript_preview[1].role,
        TranscriptRole::Assistant
    );
    assert_eq!(coordinator.state().transcript_preview[1].text, "world");
}

#[test]
fn assistant_markdown_message_is_projected_before_transcript_append() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("lists_blockquote.md");
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: markdown,
    });
    coordinator.drain_transport();

    let lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.contains(&"• one"));
    assert!(lines.contains(&"• two"));
    assert!(lines.contains(&"1. first"));
    assert!(lines.contains(&"2. second"));
    assert!(lines.contains(&"│ quoted"));
    assert!(lines.contains(&"│ second"));
}

#[test]
fn assistant_markdown_message_preserves_inline_span_styles_in_transcript_state() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "hello **bold** and `code`".to_string(),
    });
    coordinator.drain_transport();

    let line = &coordinator.state().transcript_preview[0];
    let rendered = line
        .rendered
        .as_ref()
        .expect("assistant markdown should keep rendered line");

    assert!(rendered
        .spans
        .iter()
        .any(|span| span.content.as_ref() == "bold"
            && span.style.add_modifier.contains(Modifier::BOLD)));
    assert!(rendered
        .spans
        .iter()
        .any(|span| span.content.as_ref() == "code"
            && span.style.fg == Some(CTP_MOCHA_YELLOW)
            && span.style.add_modifier.contains(Modifier::DIM)));
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
            .map(|line| (line.role, line.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::User, "h"),
            (TranscriptRole::Separator, "────────────────"),
            (TranscriptRole::Assistant, "world"),
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

    let status_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        coordinator.state(),
        "openai/gpt-4",
        "active=crossterm, crossterm=available, /dev/tty=available",
        "event from crossterm",
        None,
    );
    let joined = status_lines.join("\n");

    assert!(joined.contains("Thinking..."));
    assert!(!joined.contains(&input_before));
    assert_eq!(
        crate::commands::agent::ui::tui::runtime::input_line_for_test_at_millis(
            coordinator.state(),
            0,
        ),
        input_before
    );

    coordinator.enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
    coordinator.drain_transport();

    let status_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        coordinator.state(),
        "openai/gpt-4",
        "active=crossterm, crossterm=available, /dev/tty=available",
        "event from crossterm",
        None,
    );
    assert!(status_lines[0].contains("Idle (type and press Enter)"));
    assert!(status_lines[1].contains("Mode:"));
    assert_eq!(coordinator.state().input.buffer, input_before);
}

#[test]
fn status_lines_report_insert_and_normal_modes() {
    let mut state = AppState::new();

    let insert_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4",
        "active=crossterm",
        "event",
        None,
    );
    assert!(insert_lines[1].contains("INSERT"));

    state.enter_normal_mode();
    let normal_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4",
        "active=crossterm",
        "event",
        None,
    );
    assert!(normal_lines[1].contains("NORMAL"));
}

#[test]
fn compact_status_line_is_concise_and_includes_mode_queue_tokens_model() {
    let mut state = AppState::new();
    state.enter_normal_mode();
    state.latest_total_tokens = Some(7);
    state.session_total_tokens = 27;

    let status_line = crate::commands::agent::ui::tui::runtime::compact_status_line_for_test(
        &state,
        "openai/gpt-4o-mini",
        "active=crossterm",
        "event",
        None,
    );

    assert!(status_line.contains("NOR"));
    assert!(status_line.contains("queue: 0"));
    assert!(status_line.contains("tokens: 27"));
    assert!(status_line.contains("openai/gpt-4o-mini"));
}

#[test]
fn status_lines_report_visual_semantics_and_indicator_only_for_transcript_focus() {
    let mut state = AppState::new();
    state.transcript_preview.push(crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Assistant,
        text: "line 0".to_string(),
        rendered: None,
    });
    state.transcript_preview.push(crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Assistant,
        text: "line 1".to_string(),
        rendered: None,
    });

    state.enter_visual_mode();
    let visual_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4",
        "active=crossterm",
        "event",
        None,
    );
    assert!(visual_lines[1].contains("VISUAL"));
    assert!(
        visual_lines
            .iter()
            .any(|line| line.starts_with("Visual: transcript "))
    );
    assert!(
        visual_indicator_line_for_test(&state)
            .expect("visual indicator")
            .contains("anchor=1 cursor=1 range=1..1")
    );

    state.pane_focus = PaneFocus::Input;
    let lines_without_transcript_focus = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4",
        "active=crossterm",
        "event",
        None,
    );
    assert!(
        !lines_without_transcript_focus
            .iter()
            .any(|line| line.starts_with("Visual: transcript "))
    );
    assert!(visual_indicator_line_for_test(&state).is_none());
}

#[test]
fn transcript_title_reflects_visual_anchor_cursor_and_range() {
    let mut state = AppState::new();
    for idx in 0..4 {
        state.transcript_preview.push(crate::commands::agent::ui::tui::state::TranscriptLine {
            role: TranscriptRole::Assistant,
            text: format!("line {idx}"),
            rendered: None,
        });
    }

    state.enter_visual_mode();
    state.extend_visual_cursor_line_up();

    let title = transcript_title_for_test(&state);
    assert_eq!(title, "Transcript [VISUAL anchor=3 cursor=2 range=2..3]");

    state.pane_focus = PaneFocus::Input;
    assert_eq!(transcript_title_for_test(&state), "Transcript");
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
    let mut source = crate::commands::agent::ui::tui::runtime::CrosstermTerminalEvents::new(
        std::time::Duration::from_millis(0),
    );

    let event = source.poll_event();
    assert_eq!(event, Ok(None));
}

#[test]
fn crossterm_enter_modifier_mapping_distinguishes_submit_vs_newline_intents() {
    let plain = crate::commands::agent::ui::tui::runtime::map_crossterm_event_for_test(Event::Key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    ));
    assert_eq!(plain, Some(TerminalEvent::Key(TerminalKey::Enter)));

    let alt = crate::commands::agent::ui::tui::runtime::map_crossterm_event_for_test(Event::Key(
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        },
    ));
    assert_eq!(alt, Some(TerminalEvent::Key(TerminalKey::AltEnter)));

    let shift = crate::commands::agent::ui::tui::runtime::map_crossterm_event_for_test(Event::Key(
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        },
    ));
    assert_eq!(shift, Some(TerminalEvent::Key(TerminalKey::ShiftEnter)));
}

#[test]
fn coordinator_hydrates_transcript_from_existing_session_messages() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let mut session = store
        .get_or_create(Some("hydrate-session".to_string()))
        .expect("session");

    session
        .add_message(
            &store,
            Message::new("user".to_string(), "hello from history".to_string()),
        )
        .expect("user msg");
    session
        .add_message(
            &store,
            Message::new("assistant".to_string(), "history reply".to_string()),
        )
        .expect("assistant msg");

    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(
        session
            .messages()
            .iter()
            .map(|message| UiMessageSnapshot::new(message.role(), message.content())),
    );

    assert!(
        coordinator
            .state()
            .transcript_preview
            .iter()
            .any(|line| line.text.contains("hello from history"))
    );
    assert!(
        coordinator
            .state()
            .transcript_preview
            .iter()
            .any(|line| line.text.contains("history reply"))
    );
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
            .map(|line| (line.role, line.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (TranscriptRole::User, "line1"),
            (TranscriptRole::User, "line2"),
            (TranscriptRole::Separator, "────────────────"),
            (TranscriptRole::Assistant, "reply"),
            (TranscriptRole::Separator, "────────────────"),
            (TranscriptRole::Tool, "tool output"),
            (TranscriptRole::System, "system fallback"),
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
    assert_eq!(coordinator.state().transcript_preview[0].role, TranscriptRole::Tool);
    assert_eq!(
        coordinator.state().transcript_preview[0].text,
        "tool[k8s__list_pods] args={\"namespace\":\"prod\"} · done"
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
        .map(|line| (line.role, line.text.as_str()))
        .collect::<Vec<_>>();

    assert!(lines.contains(&(TranscriptRole::User, "# user stays literal")));
    assert!(lines.contains(&(TranscriptRole::Assistant, "heading")));
    assert!(lines.contains(&(TranscriptRole::Assistant, "x")));
}

#[test]
fn coordinator_hydration_preserves_assistant_markdown_styles() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "assistant",
        "**bold** and `code`",
    )]);

    let line = &coordinator.state().transcript_preview[0];
    let rendered = line
        .rendered
        .as_ref()
        .expect("assistant hydration should preserve rendered markdown line");

    assert!(rendered
        .spans
        .iter()
        .any(|span| span.content.as_ref() == "bold"
            && span.style.add_modifier.contains(Modifier::BOLD)));
    assert!(rendered
        .spans
        .iter()
        .any(|span| span.content.as_ref() == "code"
            && span.style.fg == Some(CTP_MOCHA_YELLOW)
            && span.style.add_modifier.contains(Modifier::DIM)));
}

#[test]
fn coordinator_hydration_keeps_unsupported_markdown_readable_in_assistant_transcript() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    let markdown = markdown_fixture("unsupported_fallback.md");
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "assistant",
        &markdown,
    )]);

    let lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| line.role == TranscriptRole::Assistant)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();

    assert!(lines.iter().any(|line| line.contains("| col | val |")));
    assert!(lines.iter().any(|line| line.contains("| a | b |")));
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
    coordinator.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new(
        "assistant",
        &markdown,
    )]);

    let assistant_lines = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| line.role == TranscriptRole::Assistant)
        .collect::<Vec<_>>();

    assert!(!assistant_lines.is_empty());
    assert!(assistant_lines.iter().any(|line| line.text.contains("fn main() {")));
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
        .filter(|line| line.text == "dup-check")
        .count();
    let assistant_count = coordinator
        .state()
        .transcript_preview
        .iter()
        .filter(|line| line.text == "dup-check-reply")
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
fn visible_transcript_window_shows_tail_when_following() {
    let mut state = AppState::new();
    for i in 0..30 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }

    let window = visible_transcript_window(
        &state.transcript_preview,
        5,
        state.transcript_scroll_lines_from_bottom,
        true,
    );

    assert_eq!(window.len(), 5);
    assert!(window[0].text.contains("line 25"));
    assert!(window[4].text.contains("line 29"));
}

#[test]
fn follow_tail_render_window_reflows_long_lines_when_narrower() {
    let mut state = AppState::new();
    state.push_transcript_line(
        TranscriptRole::Assistant,
        "abcdefghijklmnopqrstuvwxyz0123456789",
    );
    state.push_transcript_line(TranscriptRole::Assistant, "tail");

    let (_wide_start, wide_window) = visible_transcript_window_for_render_for_test(
        &state.transcript_preview,
        3,
        state.transcript_scroll_lines_from_bottom,
        true,
        40,
    );
    assert_eq!(wide_window.len(), 2);
    assert_eq!(wide_window[0].text, "abcdefghijklmnopqrstuvwxyz0123456789");
    assert_eq!(wide_window[1].text, "tail");

    let (_narrow_start, narrow_window) = visible_transcript_window_for_render_for_test(
        &state.transcript_preview,
        3,
        state.transcript_scroll_lines_from_bottom,
        true,
        8,
    );
    assert_eq!(narrow_window.len(), 1);
    assert_eq!(narrow_window[0].text, "tail");
}

#[test]
fn visible_transcript_window_respects_scroll_from_bottom_when_not_following() {
    let mut state = AppState::new();
    for i in 0..30 {
        state.push_transcript_line(TranscriptRole::Assistant, format!("line {i}"));
    }
    state.transcript_follow_tail = false;
    state.transcript_scroll_lines_from_bottom = 8;

    let window = visible_transcript_window(
        &state.transcript_preview,
        5,
        state.transcript_scroll_lines_from_bottom,
        state.transcript_follow_tail,
    );

    assert_eq!(window.len(), 5);
    assert!(window[0].text.contains("line 17"));
    assert!(window[4].text.contains("line 21"));
}

#[test]
fn follow_tail_defaults_to_latest_lines_after_each_turn() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    for turn in 0..3 {
        for ch in format!("u{turn}").chars() {
            let mut source = StubEventSource {
                next: Some(TerminalEvent::Key(TerminalKey::Char(ch))),
            };
            coordinator.pump_once(&mut source);
        }

        let mut submit = StubEventSource {
            next: Some(TerminalEvent::Key(TerminalKey::Enter)),
        };
        coordinator.pump_once(&mut submit);

        coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
            text: format!("a{turn}"),
        });
        coordinator.enqueue_ui_event(UiEvent::Completed { tool_calls: 0 });
        coordinator.drain_transport();

        let state = coordinator.state();
        assert!(state.transcript_follow_tail);
        assert_eq!(state.transcript_scroll_lines_from_bottom, 0);
        let window = visible_transcript_window(
            &state.transcript_preview,
            4,
            state.transcript_scroll_lines_from_bottom,
            state.transcript_follow_tail,
        );
        let expected = format!("a{turn}");
        assert_eq!(
            window.last().map(|line| line.text.as_str()),
            Some(expected.as_str())
        );
    }

    let state = coordinator.state();
    assert!(state.transcript_follow_tail);
    assert_eq!(state.transcript_scroll_lines_from_bottom, 0);

    let window = visible_transcript_window(
        &state.transcript_preview,
        4,
        state.transcript_scroll_lines_from_bottom,
        state.transcript_follow_tail,
    );

    assert_eq!(window.len(), 4);
    assert_eq!(window[1].text, "u2");
    assert_eq!(window[2].text, "────────────────");
    assert_eq!(window[3].text, "a2");
}

#[test]
fn prompt_indicator_tokens_are_unicode_and_stable() {
    assert_eq!(prompt_indicator_for_status_for_test(PromptStatus::Queued, 0), "•");
    assert_eq!(prompt_indicator_for_status_for_test(PromptStatus::Done, 0), "✓");
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::Cancelled, 0),
        "✕"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 0),
        "⠋"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 100),
        "⠙"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 200),
        "⠹"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 300),
        "⠸"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 400),
        "⠼"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 500),
        "⠴"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 600),
        "⠦"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 700),
        "⠧"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 800),
        "⠇"
    );
    assert_eq!(
        prompt_indicator_for_status_for_test(PromptStatus::InProgress, 900),
        "⠏"
    );
}

#[test]
fn cancelled_prompt_renders_indicator_and_strikethrough() {
    let line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "cancel me".to_string(),
        rendered: None,
    };
    let rendered = render_transcript_lines_for_test(
        line,
        Some(TranscriptLineStatus::Prompt(PromptStatus::Cancelled)),
        0,
    );
    assert_eq!(rendered.len(), 1);
    let spans = &rendered[0].spans;
    let combined = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(combined.contains("✕"));
    assert!(combined.contains("cancel me"));
    assert!(
        spans
            .iter()
            .all(|span| span.style.add_modifier.contains(Modifier::CROSSED_OUT))
    );
}

#[test]
fn prompt_lifecycle_transitions_render_expected_indicators() {
    let line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "hello".to_string(),
        rendered: None,
    };

    let queued = render_transcript_lines_for_test(
        line.clone(),
        Some(TranscriptLineStatus::Prompt(PromptStatus::Queued)),
        0,
    );
    assert!(queued[0].spans.iter().any(|span| span.content.contains("•")));

    let in_progress = render_transcript_lines_for_test(
        line.clone(),
        Some(TranscriptLineStatus::Prompt(PromptStatus::InProgress)),
        200,
    );
    assert!(in_progress[0]
        .spans
        .iter()
        .any(|span| span.content.contains("⠹")));

    let done = render_transcript_lines_for_test(
        line,
        Some(TranscriptLineStatus::Prompt(PromptStatus::Done)),
        0,
    );
    assert!(done[0].spans.iter().any(|span| span.content.contains("✓")));
}

#[test]
fn user_rows_have_subtle_accent_while_assistant_rows_remain_plain() {
    let user_line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "hello".to_string(),
        rendered: None,
    };
    let assistant_line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Assistant,
        text: "hi".to_string(),
        rendered: None,
    };

    let user = render_transcript_lines_for_test(user_line, None, 0);
    let assistant = render_transcript_lines_for_test(assistant_line, None, 0);

    assert!(user[0].spans.iter().any(|span| span.content.as_ref() == "  "));
    assert!(assistant[0].spans.iter().any(|span| span.content.as_ref() == "  "));
}

#[test]
fn lane_prefix_builder_composes_distinct_prefix_spans_by_role() {
    let user = lane_prefix_spans_for_test(TranscriptRole::User, false);
    let assistant = lane_prefix_spans_for_test(TranscriptRole::Assistant, false);
    let tool = lane_prefix_spans_for_test(TranscriptRole::Tool, false);

    let user_text = user
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let assistant_text = assistant
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let tool_text = tool
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(user_text, "  ▏ ");
    assert_eq!(assistant_text, "    ");
    assert_eq!(tool_text, "  ⚒ ");
}

#[test]
fn row_span_builder_enforces_role_status_and_tool_metadata_style_channels() {
    let tool = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"} · done".to_string(),
        rendered: None,
    };
    let tool_spans = row_spans_for_test(
        tool,
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done)),
        false,
        false,
        0,
    );

    let indicator = tool_spans
        .iter()
        .find(|span| span.content.as_ref().contains('✓'))
        .expect("status indicator span");
    assert_eq!(indicator.style.fg, Some(CTP_MOCHA_GREEN));
    assert!(!indicator.style.add_modifier.contains(Modifier::DIM));

    let metadata = tool_spans
        .iter()
        .find(|span| span.content.as_ref().contains("args={\"namespace\":\"prod\"}"))
        .expect("tool metadata span");
    assert_eq!(metadata.style.fg, Some(CTP_MOCHA_OVERLAY1));
    assert!(metadata.style.add_modifier.contains(Modifier::DIM));

    let user = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "hello".to_string(),
        rendered: None,
    };
    let user_spans = row_spans_for_test(
        user,
        Some(TranscriptLineStatus::Prompt(PromptStatus::Done)),
        false,
        false,
        0,
    );
    let user_content = user_spans
        .iter()
        .find(|span| span.content.as_ref() == "hello")
        .expect("user content span");
    assert_eq!(user_content.style.fg, Some(CTP_MOCHA_BLUE));
}

#[test]
fn row_span_builder_applies_distinct_role_background_channels() {
    let user = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "user line".to_string(),
        rendered: None,
    };
    let assistant = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Assistant,
        text: "assistant line".to_string(),
        rendered: None,
    };
    let tool = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"}".to_string(),
        rendered: None,
    };

    let user_spans = row_spans_for_test(user, None, false, false, 0);
    let assistant_spans = row_spans_for_test(assistant, None, false, false, 0);
    let tool_spans = row_spans_for_test(tool, None, false, false, 0);

    assert!(user_spans.iter().all(|span| span.style.bg == Some(CTP_MOCHA_SURFACE0)));
    assert!(assistant_spans.iter().all(|span| span.style.bg.is_none()));
    assert!(tool_spans.iter().all(|span| span.style.bg.is_none()));
}

#[test]
fn role_background_extends_to_fill_row_width_for_paragraph_block_effect() {
    let user_line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "u".to_string(),
        rendered: None,
    };
    let assistant_line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Assistant,
        text: "a".to_string(),
        rendered: None,
    };

    let user_rendered = render_transcript_lines_with_flags_for_test(
        user_line,
        None,
        false,
        false,
        20,
        0,
    );
    let assistant_rendered = render_transcript_lines_with_flags_for_test(
        assistant_line,
        None,
        false,
        false,
        20,
        0,
    );

    let user_pad = user_rendered[0]
        .spans
        .last()
        .expect("user line should include trailing pad span");
    assert!(user_pad.content.chars().count() > 0);
    assert_eq!(user_pad.style.bg, Some(CTP_MOCHA_SURFACE0));

    let assistant_pad = assistant_rendered[0]
        .spans
        .last()
        .expect("assistant line should include trailing pad span");
    assert!(assistant_pad.content.chars().count() > 0);
    assert!(assistant_pad.style.bg.is_none());
}

#[test]
fn submitted_multiline_prompt_preserves_line_breaks_in_transcript_render() {
    let line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "line one\nline two\nline three".to_string(),
        rendered: None,
    };

    let rendered = render_transcript_lines_with_flags_for_test(
        line,
        Some(TranscriptLineStatus::Prompt(PromptStatus::Done)),
        false,
        false,
        60,
        0,
    );

    assert_eq!(rendered.len(), 3);

    let first = rendered[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let second = rendered[1]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let third = rendered[2]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(first.contains("line one"));
    assert!(second.contains("line two"));
    assert!(third.contains("line three"));
    assert!(first.contains('✓'));
    assert!(!second.contains('✓'));
    assert!(!third.contains('✓'));
}

#[test]
fn selection_overlay_is_applied_last_to_all_style_channels() {
    let line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"}".to_string(),
        rendered: None,
    };
    let spans = row_spans_for_test(
        line,
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress)),
        false,
        true,
        0,
    );

    assert!(spans
        .iter()
        .all(|span| span.style.bg == Some(CTP_MOCHA_SURFACE1)));
}

#[test]
fn selection_overlay_remains_legible_on_user_lane_prefix_rows() {
    let user_line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "selected".to_string(),
        rendered: None,
    };

    let rendered = render_transcript_lines_with_flags_for_test(
        user_line,
        Some(TranscriptLineStatus::Prompt(PromptStatus::Done)),
        true,
        false,
        80,
        0,
    );
    let lane = rendered[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "▏ ")
        .expect("lane span");

    assert_eq!(lane.style.bg, Some(CTP_MOCHA_SURFACE1));
    assert_eq!(lane.style.fg, Some(CTP_MOCHA_BLUE));
}

#[test]
fn cursor_marker_coexists_with_user_lane_prefix_and_status_indicator() {
    let user_line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::User,
        text: "cursor line".to_string(),
        rendered: None,
    };

    let rendered = render_transcript_lines_with_flags_for_test(
        user_line,
        Some(TranscriptLineStatus::Prompt(PromptStatus::InProgress)),
        false,
        true,
        80,
        200,
    );

    assert!(rendered[0].spans.iter().any(|span| span.content.as_ref() == "> "));
    assert!(rendered[0].spans.iter().any(|span| span.content.as_ref() == "▏ "));
    assert!(rendered[0].spans.iter().any(|span| span.content.contains("⠹")));
}

#[test]
fn tool_row_renders_spinner_while_running_and_done_on_end() {
    let line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"}".to_string(),
        rendered: None,
    };

    let running_0 = render_transcript_lines_for_test(
        line.clone(),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress)),
        0,
    );
    let running_1 = render_transcript_lines_for_test(
        line.clone(),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress)),
        100,
    );
    let running_2 = render_transcript_lines_for_test(
        line.clone(),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress)),
        200,
    );
    assert!(running_0[0].spans.iter().any(|span| span.content.contains("⠋")));
    assert!(running_1[0].spans.iter().any(|span| span.content.contains("⠙")));
    assert!(running_2[0].spans.iter().any(|span| span.content.contains("⠹")));

    let done = render_transcript_lines_for_test(
        line,
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done)),
        0,
    );
    assert!(done[0].spans.iter().any(|span| span.content.contains("✓")));
}

#[test]
fn tool_rows_render_structured_label_and_dimmed_metadata() {
    let line = crate::commands::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::Tool,
        text: "tool[k8s__list_pods] args={\"namespace\":\"prod\"} · done".to_string(),
        rendered: None,
    };

    let rendered = render_transcript_lines_for_test(
        line,
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done)),
        0,
    );

    assert!(rendered[0]
        .spans
        .iter()
        .any(|span| span.content.as_ref() == "tool[k8s__list_pods]"));
    let meta = rendered[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref().contains("args={\"namespace\":\"prod\"} · done"))
        .expect("tool metadata span");
    assert_eq!(meta.style.fg, Some(CTP_MOCHA_OVERLAY1));
    assert!(meta.style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn indicator_style_tokens_map_prompt_and_tool_statuses_semantically() {
    let prompt_queued = indicator_style_for_status_for_test(TranscriptLineStatus::Prompt(PromptStatus::Queued));
    let prompt_running = indicator_style_for_status_for_test(TranscriptLineStatus::Prompt(PromptStatus::InProgress));
    let prompt_done = indicator_style_for_status_for_test(TranscriptLineStatus::Prompt(PromptStatus::Done));
    let prompt_cancelled = indicator_style_for_status_for_test(TranscriptLineStatus::Prompt(PromptStatus::Cancelled));

    let tool_running = indicator_style_for_status_for_test(TranscriptLineStatus::Tool(ToolCallStatus::InProgress));
    let tool_done = indicator_style_for_status_for_test(TranscriptLineStatus::Tool(ToolCallStatus::Done));
    let tool_failed = indicator_style_for_status_for_test(TranscriptLineStatus::Tool(ToolCallStatus::Failed));

    assert_eq!(prompt_queued.fg, Some(CTP_MOCHA_OVERLAY0));
    assert_eq!(prompt_running.fg, Some(CTP_MOCHA_SAPPHIRE));
    assert_eq!(prompt_done.fg, Some(CTP_MOCHA_GREEN));
    assert_eq!(prompt_cancelled.fg, Some(CTP_MOCHA_OVERLAY0));

    assert_eq!(tool_running.fg, Some(CTP_MOCHA_SAPPHIRE));
    assert_eq!(tool_done.fg, Some(CTP_MOCHA_GREEN));
    assert_eq!(tool_failed.fg, Some(CTP_MOCHA_RED));
}

#[test]
fn transition_spacing_matrix_is_deterministic_for_role_changes() {
    assert!(!transition_spacer_for_roles_for_test(None, TranscriptRole::User));
    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::User), TranscriptRole::User));
    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::Assistant), TranscriptRole::Assistant));
    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::Tool), TranscriptRole::Tool));

    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::User), TranscriptRole::Assistant));
    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::Assistant), TranscriptRole::User));

    assert!(transition_spacer_for_roles_for_test(Some(TranscriptRole::Assistant), TranscriptRole::Tool));
    assert!(transition_spacer_for_roles_for_test(Some(TranscriptRole::Tool), TranscriptRole::Assistant));
    assert!(transition_spacer_for_roles_for_test(Some(TranscriptRole::User), TranscriptRole::Tool));
    assert!(transition_spacer_for_roles_for_test(Some(TranscriptRole::Tool), TranscriptRole::User));

    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::Separator), TranscriptRole::Assistant));
    assert!(!transition_spacer_for_roles_for_test(Some(TranscriptRole::Assistant), TranscriptRole::Separator));
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
    assert_eq!(statuses, vec![PromptStatus::Cancelled, PromptStatus::Cancelled]);

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
fn manual_scroll_up_pauses_follow_tail_and_bottom_resume_restores_it() {
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    for i in 0..30 {
        coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
            text: format!("line {i}"),
        });
    }
    coordinator.drain_transport();

    let mut page_up = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::PageUp)),
    };
    coordinator.pump_once(&mut page_up);

    assert!(!coordinator.state().transcript_follow_tail);
    assert!(coordinator.state().transcript_cursor_index().is_some());
    assert!(coordinator.state().transcript_cursor_index().unwrap_or(0) < coordinator.state().transcript_preview.len().saturating_sub(1));

    coordinator.enqueue_ui_event(UiEvent::AssistantMessage {
        text: "line after scroll".to_string(),
    });
    coordinator.drain_transport();
    assert!(coordinator.state().transcript_follow_tail);
    assert_eq!(coordinator.state().transcript_scroll_lines_from_bottom, 0);

    let mut page_down = StubEventSource {
        next: Some(TerminalEvent::Key(TerminalKey::PageDown)),
    };
    coordinator.pump_once(&mut page_down);

    assert!(coordinator.state().transcript_follow_tail);
    assert_eq!(coordinator.state().transcript_scroll_lines_from_bottom, 0);

    let state = coordinator.state();
    let window = visible_transcript_window(
        &state.transcript_preview,
        3,
        state.transcript_scroll_lines_from_bottom,
        state.transcript_follow_tail,
    );
    assert_eq!(window.len(), 3);
    assert_eq!(window[2].text, "line after scroll");
}

#[test]
fn main_pane_vertical_split_has_no_overlap_or_bottom_cutoff() {
    let (_header, transcript, status, input) = RuntimeCoordinator::main_pane_rects_for_height(9);

    assert!(
        transcript.height > 0,
        "transcript pane should remain visible"
    );
    assert_eq!(transcript.y + transcript.height, status.y);
    assert_eq!(status.y + status.height, input.y);
    assert_eq!(input.y + input.height, 9);
}

#[test]
fn multiline_input_prompt_icon_appears_only_on_first_visual_row() {
    let mut state = AppState::new();
    state.input.buffer = "ab\n12345".to_string();

    let rows = input_rows_with_prompt_for_test(&state, 5);
    assert_eq!(rows, vec!["❯ ab", "  123", "  45"]);
}

#[test]
fn status_lines_include_stable_active_model_identity_line() {
    let state = AppState::new();
    let status_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
        "active=crossterm, crossterm=available, /dev/tty=available",
        "event from crossterm",
        None,
    );

    assert!(
        status_lines
            .iter()
            .any(|line| line == "Model: openai/gpt-4o-mini")
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line.starts_with("Input backend:"))
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line.starts_with("Input poll:"))
    );
    assert!(
        status_lines
            .iter()
            .any(|line| line.starts_with("Input error:"))
    );
}

#[test]
fn status_lines_include_tokens_line_with_na_before_any_llm_end() {
    let state = AppState::new();
    let status_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        &state,
        "openai/gpt-4o-mini",
        "active=crossterm, crossterm=available, /dev/tty=available",
        "event from crossterm",
        None,
    );

    assert!(status_lines.iter().any(|line| line == "Tokens: n/a"));
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

    let status_lines = crate::commands::agent::ui::tui::runtime::status_lines_for_test(
        coordinator.state(),
        "openai/gpt-4o-mini",
        "active=crossterm, crossterm=available, /dev/tty=available",
        "event from crossterm",
        None,
    );

    assert!(
        status_lines
            .iter()
            .any(|line| line == "Tokens: in=3 out=4 total=7 session=27")
    );
}

#[test]
fn compact_status_line_reports_session_total_tokens_only() {
    let mut state = AppState::new();
    state.latest_total_tokens = Some(7);
    state.session_total_tokens = 27;

    let status_line = crate::commands::agent::ui::tui::runtime::compact_status_line_for_test(
        &state,
        "openai/gpt-4o-mini",
        "active=crossterm",
        "event",
        None,
    );

    assert!(status_line.contains("tokens: 27"));
    assert!(!status_line.contains("3/4/7"));
    assert!(!status_line.contains("in="));
    assert!(!status_line.contains("out="));
}

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
