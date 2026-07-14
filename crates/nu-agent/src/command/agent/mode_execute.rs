use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nu_protocol::{LabeledError, Value};

use nu_agent_a2a::{A2aCompletionEvent, IncomingTask, Part};
use nu_agent_core::{
    conversation::runtime::AgentConversationRuntime,
    orchestrator::{
        run_hydrated_interactive_loop_with_external_prompts,
        run_interactive_loop_with_external_prompts, run_single_turn,
    },
    policy::UiPolicy,
    protocol::{
        contracts::UiMessageSnapshot, event::PermissionDecision, mcp_management::HasMcpManagement,
    },
};
use nu_agent_tty::StderrProgressUi;
use nu_agent_tty::{StderrUiFactory, UiRendererFactory};
use nu_agent_tui::TuiInteractiveUi;
use nu_agent_tui::platform::terminal::TerminalLifecycle;
use nu_agent_tui::runtime::{
    AnsiTerminalBackend, HybridTerminalEvents, TtyTerminalEvents, TuiRuntimeRenderer,
    open_tty_reader, run_with_terminal_restore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentMode {
    Tui,
    Stderr,
}

impl AgentMode {
    pub(crate) fn is_tui(self) -> bool {
        matches!(self, Self::Tui)
    }
}

/// Whether the plugin should call `enter_foreground()` to receive SIGINT.
/// True for TUI (always needs it) and for stderr mode when stderr is a TTY
/// (user has a terminal and may press Ctrl+C).
pub(crate) fn should_enter_foreground(mode: AgentMode, stderr_is_tty: bool) -> bool {
    mode.is_tui() || stderr_is_tty
}

pub(crate) fn resolve_agent_mode(
    input_is_nothing: bool,
    stdin_is_tty: bool,
    stderr_is_tty: bool,
) -> AgentMode {
    if input_is_nothing && stdin_is_tty && stderr_is_tty {
        AgentMode::Tui
    } else {
        AgentMode::Stderr
    }
}

/// Hydration context for TUI transcript replay on session attach.
pub(crate) struct TuiHydrationInput {
    pub should_hydrate: bool,
    pub initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_tui_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
    a2a_task_rx: Option<tokio::sync::mpsc::Receiver<IncomingTask>>,
    a2a_completion_rx: Option<tokio::sync::mpsc::Receiver<A2aCompletionEvent>>,
) -> Result<Value, LabeledError> {
    // Set up the interactive permission pending map for TUI mode.
    // This Arc is shared between the worker thread (via InteractivePermissionResolver)
    // and the main thread (via the orchestrator's permission poll loop).
    let pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    runtime_impl.interactive_pending = Some(Arc::clone(&pending));

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
        runtime_impl.provider_name(),
        runtime_impl.model(),
    );
    tui_ui.set_active_model_identity(active_model_identity.clone());
    let model_picker_catalog =
        super::picker::model_picker_catalog_from_cached_startup_plugin_config(
            runtime_impl.startup_plugin_config(),
            active_model_identity.as_str(),
        );
    tui_ui.set_model_picker_options(model_picker_catalog);
    let agent_picker_catalog = super::picker::build_agent_picker_catalog(
        runtime_impl.available_agent_summaries(),
        runtime_impl.agent_identity(),
    );
    tui_ui.set_agent_picker_options(agent_picker_catalog);
    let cycle_names: Vec<String> = runtime_impl
        .available_agent_summaries()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    tui_ui.set_agent_cycle_names(cycle_names);
    if let Some(identity) = runtime_impl.agent_identity() {
        tui_ui.set_active_agent_identity(identity);
    }
    let caller_cwd = runtime_impl.mcp_caller_cwd().map(|p| p.to_path_buf());
    tui_ui.set_repo_branch_caller_cwd(caller_cwd.clone());
    match caller_cwd {
        Some(cwd) => {
            let skills = nu_agent_core::protocol::skills::discover_skill_catalog_for_cwd(&cwd);
            tui_ui.set_skills_projection(skills);
        }
        None => tui_ui.mark_skills_discovery_failed(),
    }
    tui_ui.set_mcp_lifecycle_projection(runtime_impl.mcp_lifecycle_projection().to_vec());
    tui_ui.set_llm_visible_mcp_tool_count(runtime_impl.llm_visible_mcp_tool_count());
    tui_ui.set_context_window_max_tokens(runtime_impl.max_context_tokens());

    // Bridge A2A channels (incoming tasks + completion events) into a single
    // std channel of formatted prompt strings that the orchestrator can poll
    // without A2A knowledge.
    let external_prompt_rx: Option<std::sync::mpsc::Receiver<String>> = {
        let (tx, std_rx) = std::sync::mpsc::channel::<String>();
        let mut has_sources = false;

        // Forward incoming A2A tasks
        if let Some(mut rx) = a2a_task_rx {
            has_sources = true;
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Some(incoming) = rx.blocking_recv() {
                    let text: String = incoming
                        .message
                        .parts
                        .iter()
                        .filter_map(|p| match p {
                            Part::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let prompt = format!(
                        "[A2A Task {} from {}]: {}\n\nWhen you have processed this request, call `tasks.complete` with taskId `{}` and your response as the result.",
                        incoming.task_id, incoming.sender_url, text, incoming.task_id
                    );
                    if tx.send(prompt).is_err() {
                        break; // std receiver dropped, no more forwarding needed
                    }
                }
            });
        }

        // Forward A2A completion events
        if let Some(mut rx) = a2a_completion_rx {
            has_sources = true;
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Some(event) = rx.blocking_recv() {
                    let prompt = format!(
                        "[A2A Task {} completed by {}]: {}\n\nStatus: {}. Use tasks.get to inspect the full task details.",
                        event.task_id, event.agent_name, event.result, event.status
                    );
                    if tx.send(prompt).is_err() {
                        break;
                    }
                }
            });
        }

        if has_sources { Some(std_rx) } else { None }
    };

    let result = run_with_terminal_restore(&mut terminal_lifecycle, || {
        if input_is_nothing {
            if hydration.should_hydrate {
                run_hydrated_interactive_loop_with_external_prompts(
                    runtime_impl,
                    &mut tui_ui,
                    hydration.initial_messages,
                    hydration.last_total_tokens,
                    span,
                    Some(Arc::clone(&pending)),
                    external_prompt_rx,
                )
            } else {
                run_interactive_loop_with_external_prompts(
                    runtime_impl,
                    &mut tui_ui,
                    span,
                    Some(Arc::clone(&pending)),
                    external_prompt_rx,
                )
            }
        } else {
            // Non-interactive: run a single turn with the provided input.
            // A2A tasks are not processed in this mode (single-turn).
            let (prompt, context) = super::input::extract_prompt_and_context(input)?;
            run_single_turn(runtime_impl, &mut tui_ui, prompt, context, span)
        }
    });

    super::run_command::map_tui_run_result(result)
}

pub(crate) fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
    mut a2a_task_rx: Option<tokio::sync::mpsc::Receiver<IncomingTask>>,
    mut a2a_completion_rx: Option<tokio::sync::mpsc::Receiver<A2aCompletionEvent>>,
) -> Result<Value, LabeledError> {
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Spawn a tokio task that awaits SIGINT and sets the cancel flag
    let signal_flag = Arc::clone(&cancel_flag);
    runtime_impl.spawn(async move {
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

    // Check for A2A completion events first (highest priority).
    if let Some(ref mut rx) = a2a_completion_rx
        && let Ok(event) = rx.try_recv()
    {
        let prompt = format!(
            "[A2A Task {} completed by {}]: {}\n\nStatus: {}. Use tasks.get to inspect the full task details.",
            event.task_id, event.agent_name, event.result, event.status
        );
        return run_single_turn(runtime_impl, &mut stderr_ui, prompt, None, span);
    }

    // Then check for pending A2A incoming tasks.
    if let Some(ref mut rx) = a2a_task_rx
        && let Ok(incoming) = rx.try_recv()
    {
        let text: String = incoming
            .message
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        let prompt = format!(
            "[A2A Task {} from {}]: {}\n\nWhen you have processed this request, call `tasks.complete` with taskId `{}` and your response as the result.",
            incoming.task_id, incoming.sender_url, text, incoming.task_id
        );
        return run_single_turn(runtime_impl, &mut stderr_ui, prompt, None, span);
    }

    let (prompt, context) = super::input::extract_prompt_and_context(input)?;
    run_single_turn(runtime_impl, &mut stderr_ui, prompt, context, span)
}
