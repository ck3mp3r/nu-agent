use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nu_protocol::{LabeledError, Value};

use crate::agent::{
    application::{
        orchestrator::{run_hydrated_interactive_loop, run_interactive_loop, run_single_turn},
        ui_runtime::{StderrProgressUi, TuiInteractiveUi},
    },
    conversation::runtime::AgentConversationRuntime,
    protocol::contracts::{ExtendedRuntime, UiMessageSnapshot},
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

/// Hydration context for TUI transcript replay on session attach.
pub(crate) struct TuiHydrationInput {
    pub should_hydrate: bool,
    pub initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

pub(crate) fn run_tui_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
) -> Result<Value, LabeledError> {
    let mut terminal_lifecycle =
        TerminalLifecycle::new(AnsiTerminalBackend::new(std::io::stderr()));

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
    let active_model_identity = super::picker::format_active_model_identity(
        &runtime_impl.config.provider,
        &runtime_impl.config.model,
    );
    tui_ui.set_active_model_identity(active_model_identity.clone());
    let model_picker_catalog =
        super::picker::model_picker_catalog_from_cached_startup_plugin_config(
            runtime_impl.startup_plugin_config.as_ref(),
            active_model_identity.as_str(),
        );
    tui_ui.set_model_picker_options(model_picker_catalog);
    let agent_picker_catalog = super::picker::build_agent_picker_catalog(
        &runtime_impl.available_agent_summaries,
        runtime_impl.persona_state.agent_identity.as_deref(),
    );
    tui_ui.set_agent_picker_options(agent_picker_catalog);
    let cycle_names: Vec<String> = runtime_impl
        .available_agent_summaries
        .iter()
        .map(|s| s.name.clone())
        .collect();
    tui_ui.set_agent_cycle_names(cycle_names);
    if let Some(ref identity) = runtime_impl.persona_state.agent_identity {
        tui_ui.set_active_agent_identity(identity);
    }
    let caller_cwd = runtime_impl.mcp_state.mcp_caller_cwd.clone();
    tui_ui.set_repo_branch_caller_cwd(caller_cwd.clone());
    match caller_cwd {
        Some(cwd) => {
            let skills = crate::agent::protocol::skills::discover_skill_catalog_for_cwd(&cwd);
            tui_ui.set_skills_projection(skills);
        }
        None => tui_ui.mark_skills_discovery_failed(),
    }
    tui_ui.set_mcp_lifecycle_projection(runtime_impl.mcp_state.mcp_lifecycle_projection.clone());
    tui_ui.set_llm_visible_mcp_tool_count(runtime_impl.llm_visible_mcp_tool_count());
    tui_ui.set_context_window_max_tokens(runtime_impl.config.max_context_tokens.map(u64::from));

    let result = run_with_terminal_restore(&mut terminal_lifecycle, || {
        if input_is_nothing {
            let mailbox_rx = runtime_impl.mailbox_rx.take();
            if hydration.should_hydrate {
                run_hydrated_interactive_loop(
                    runtime_impl,
                    &mut tui_ui,
                    hydration.initial_messages,
                    hydration.last_total_tokens,
                    mailbox_rx,
                    span,
                )
            } else {
                run_interactive_loop(runtime_impl, &mut tui_ui, mailbox_rx, span)
            }
        } else {
            let (prompt, context) = super::input::extract_prompt_and_context(input)?;
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
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Spawn a tokio task that awaits SIGINT and sets the cancel flag
    let signal_flag = Arc::clone(&cancel_flag);
    runtime_impl.runtime.spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal_flag.store(true, Ordering::SeqCst);
            }
        }
    });

    let mut stderr_ui = StderrProgressUi::new(
        StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy),
        cancel_flag,
    );
    let (prompt, context) = super::input::extract_prompt_and_context(input)?;
    run_single_turn(runtime_impl, &mut stderr_ui, prompt, context, span)
}
