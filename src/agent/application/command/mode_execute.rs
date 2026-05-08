use std::time::Duration;

use nu_protocol::{LabeledError, Value};

use crate::agent::{
    application::{
        orchestrator::{run_hydrated_interactive_loop, run_interactive_loop, run_single_turn},
        ui_runtime::{StderrProgressUi, TuiInteractiveUi},
    },
    conversation::runtime::AgentConversationRuntime,
    protocol::contracts::UiMessageSnapshot,
    ui::{
        factory::{StderrUiFactory, UiRendererFactory},
        policy::UiPolicy,
        tui::{
            platform::terminal::TerminalLifecycle,
            runtime::{
                AnsiTerminalBackend, HybridTerminalEvents, TtyTerminalEvents, TuiRuntimeRenderer,
                open_tty_reader, run_with_terminal_restore,
            },
        },
    },
};

pub(crate) fn run_tui_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    tui_should_hydrate_transcript: bool,
    tui_initial_messages: Vec<UiMessageSnapshot>,
) -> Result<Value, LabeledError> {
    let mut terminal_lifecycle = TerminalLifecycle::new(AnsiTerminalBackend::new(std::io::stderr()));

    let (columns, rows) = crossterm::terminal::size().unwrap_or((120, 30));
    let fallback_events = open_tty_reader()
        .ok()
        .and_then(|tty_reader| TtyTerminalEvents::new(tty_reader, Duration::from_millis(30)).ok());

    let runtime_renderer = TuiRuntimeRenderer::new_live(
        StderrUiFactory::new(std::io::stderr(), false).create(ui_policy),
        HybridTerminalEvents::new(Duration::from_millis(60), fallback_events),
        columns,
        rows,
    )
    .map_err(|err| LabeledError::new(format!("Failed to initialize TUI renderer: {err}")))?;

    let mut tui_ui = TuiInteractiveUi::new(runtime_renderer);
    tui_ui.set_active_model_identity(super::format_active_model_identity(
        &runtime_impl.config.provider,
        &runtime_impl.config.model,
    ));

    let result = run_with_terminal_restore(&mut terminal_lifecycle, || {
        if input_is_nothing {
            if tui_should_hydrate_transcript {
                run_hydrated_interactive_loop(runtime_impl, &mut tui_ui, tui_initial_messages, span)
            } else {
                run_interactive_loop(runtime_impl, &mut tui_ui, span)
            }
        } else {
            let (prompt, context) = super::extract_prompt_and_context(input)?;
            run_single_turn(runtime_impl, &mut tui_ui, prompt, context, span)
        }
    });

    super::map_tui_run_result(result)
}

pub(crate) fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
) -> Result<Value, LabeledError> {
    let mut stderr_ui =
        StderrProgressUi::new(StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy));
    let (prompt, context) = super::extract_prompt_and_context(input)?;
    run_single_turn(runtime_impl, &mut stderr_ui, prompt, context, span)
}
