use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use nu_protocol::{LabeledError, Span, Value};

use crate::agent::protocol::{
    cancellation::is_llm_call_cancelled,
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    contracts::{
        ConversationRuntime, InteractiveUi, McpToggleRequest, McpUsabilityState,
        ProgressUi, UiMessageSnapshot,
        SharedUiAction,
    },
    event::UiEvent,
    slash::{SlashCommand, SlashParseResult, parse_slash_command},
};

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

type McpToggleResult = (Result<McpUsabilityState, String>, usize);
type PendingMcpToggle = (String, mpsc::Receiver<McpToggleResult>);
type PendingAutoCompaction = mpsc::Receiver<Option<String>>;
type PendingCompactionTrigger = mpsc::Receiver<Option<String>>;

enum WorkerCommand {
    ExecuteTurn { prompt: String, span: Span },
    EvaluateAutoCompaction {
        response_tx: mpsc::Sender<Option<String>>,
    },
    ExecuteCompactionTrigger {
        source: CompactionTriggerSource,
        response_tx: mpsc::Sender<Option<String>>,
    },
    ToggleMcp {
        server_name: String,
        enable: bool,
        response_tx: mpsc::Sender<McpToggleResult>,
    },
    Shutdown,
}

struct WorkerProgressUi {
    events: mpsc::Sender<UiEvent>,
    cancel_requested: Arc<AtomicBool>,
}

impl ProgressUi for WorkerProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        let _ = self.events.send(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_requested.swap(false, Ordering::SeqCst)
    }

    fn cancellation_value(&self, span: Span) -> Option<Value> {
        Some(Value::nothing(span))
    }
}

pub(crate) fn run_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime + Send,
    U: InteractiveUi,
{
    let mut last_authoritative_visible_count = runtime.llm_visible_mcp_tool_count();

    std::thread::scope(|scope| {
        let (worker_cmd_tx, worker_cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (worker_event_tx, worker_event_rx) = mpsc::channel::<UiEvent>();
        let (worker_result_tx, worker_result_rx) =
            mpsc::channel::<Result<Value, LabeledError>>();
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel_requested);

        let worker = scope.spawn(move || {
            let mut worker_ui = WorkerProgressUi {
                events: worker_event_tx,
                cancel_requested: worker_cancel,
            };

            while let Ok(command) = worker_cmd_rx.recv() {
                match command {
                    WorkerCommand::ExecuteTurn { prompt, span } => {
                        let result = runtime.execute_turn(&mut worker_ui, prompt, None, span);
                        let _ = worker_result_tx.send(result);
                    }
                    WorkerCommand::EvaluateAutoCompaction { response_tx } => {
                        let warning = match runtime.evaluate_auto_compaction() {
                            Some(CompactionTriggerDecision::Fire { source, .. }) => runtime
                                .execute_compaction_trigger(&mut worker_ui, source)
                                .err()
                                .map(|_error| COMPACTION_FAILURE_WARNING.to_string()),
                            _ => None,
                        };
                        let _ = response_tx.send(warning);
                    }
                    WorkerCommand::ExecuteCompactionTrigger {
                        source,
                        response_tx,
                    } => {
                        let warning = runtime
                            .execute_compaction_trigger(&mut worker_ui, source)
                            .err()
                            .map(|_error| COMPACTION_FAILURE_WARNING.to_string());
                        let _ = response_tx.send(warning);
                    }
                    WorkerCommand::ToggleMcp {
                        server_name,
                        enable,
                        response_tx,
                    } => {
                        let result = runtime.set_mcp_server_enabled(&server_name, enable);
                        let visible_count = runtime.llm_visible_mcp_tool_count();
                        let _ = response_tx.send((result, visible_count));
                    }
                    WorkerCommand::Shutdown => break,
                }
            }
        });

        let mut worker_active = false;
        let mut pending_mcp_toggles: Vec<PendingMcpToggle> = Vec::new();
        let mut pending_auto_compaction: Option<PendingAutoCompaction> = None;
        let mut pending_compaction_trigger: Option<PendingCompactionTrigger> = None;

        loop {
            ui.pump_once();

            while let Some(McpToggleRequest { server_name, enable }) = ui.take_next_mcp_toggle_request() {
                let (response_tx, response_rx) = mpsc::channel();
                let send_result = worker_cmd_tx.send(WorkerCommand::ToggleMcp {
                    server_name: server_name.clone(),
                    enable,
                    response_tx,
                });

                if send_result.is_err() {
                    ui.set_mcp_server_state_with_details(
                        &server_name,
                        McpUsabilityState::Failed,
                        Some("worker channel closed".to_string()),
                        last_authoritative_visible_count,
                    );
                    continue;
                }

                pending_mcp_toggles.push((server_name, response_rx));
            }

            let mut retained = Vec::new();
            for (server_name, response_rx) in pending_mcp_toggles.drain(..) {
                match response_rx.try_recv() {
                    Ok((Ok(state), visible_count)) => {
                        last_authoritative_visible_count = visible_count;
                        ui.set_mcp_server_state_with_details(
                            &server_name,
                            state,
                            None,
                            visible_count,
                        )
                    }
                    Ok((Err(err), visible_count)) => {
                        last_authoritative_visible_count = visible_count;
                        ui.set_mcp_server_state_with_details(
                            &server_name,
                            McpUsabilityState::Failed,
                            Some(err),
                            visible_count,
                        )
                    }
                    Err(mpsc::TryRecvError::Empty) => retained.push((server_name, response_rx)),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        ui.set_mcp_server_state_with_details(
                            &server_name,
                            McpUsabilityState::Failed,
                            Some("toggle worker disconnected".to_string()),
                            last_authoritative_visible_count,
                        )
                    }
                }
            }
            pending_mcp_toggles = retained;

            if !worker_active {
                if let Some(response_rx) = pending_compaction_trigger.take() {
                    match response_rx.try_recv() {
                        Ok(Some(message)) => ui.emit(&UiEvent::Warning { message }),
                        Ok(None) => {}
                        Err(mpsc::TryRecvError::Empty) => {
                            pending_compaction_trigger = Some(response_rx)
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            ui.emit(&UiEvent::Warning {
                                message: "Compaction worker disconnected".to_string(),
                            });
                        }
                    }
                }

                if pending_auto_compaction.is_none() {
                    let (response_tx, response_rx) = mpsc::channel();
                    if worker_cmd_tx
                        .send(WorkerCommand::EvaluateAutoCompaction { response_tx })
                        .is_ok()
                    {
                        pending_auto_compaction = Some(response_rx);
                    }
                }

                if let Some(response_rx) = pending_auto_compaction.take() {
                    match response_rx.try_recv() {
                        Ok(Some(message)) => ui.emit(&UiEvent::Warning { message }),
                        Ok(None) => {}
                        Err(mpsc::TryRecvError::Empty) => {
                            pending_auto_compaction = Some(response_rx)
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {}
                    }
                }
            } else {
                pending_auto_compaction = None;
                pending_compaction_trigger = None;
            }

            if ui.take_cancel_requested() {
                cancel_requested.store(true, Ordering::SeqCst);
            }

            while let Ok(event) = worker_event_rx.try_recv() {
                ui.emit(&event);
            }

            while let Ok(result) = worker_result_rx.try_recv() {
                worker_active = false;
                match result {
                    Ok(_) => {}
                    Err(error) if is_llm_call_cancelled(&error) => {}
                    Err(error) => {
                        let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                        return Err(error);
                    }
                }
            }

            if let Some(error) = ui.fatal_error() {
                let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                return Err(LabeledError::new(format!("Interactive UI failed: {error}")));
            }

            if !worker_active {
                while let Some(prompt) = ui.take_submitted_prompt() {
                    if prompt.trim().is_empty() {
                        continue;
                    }

                    match parse_slash_command(&prompt) {
                        SlashParseResult::Command(SlashCommand::Compact) => {
                            let (response_tx, response_rx) = mpsc::channel();
                            if worker_cmd_tx
                                .send(WorkerCommand::ExecuteCompactionTrigger {
                                    source: CompactionTriggerSource::SlashCompact,
                                    response_tx,
                                })
                                .is_ok()
                            {
                                pending_compaction_trigger = Some(response_rx);
                            } else {
                                ui.emit(&UiEvent::Warning {
                                    message: "Compaction worker channel closed".to_string(),
                                });
                            }
                            continue;
                        }
                        SlashParseResult::Command(SlashCommand::Help) => {
                            let _ = ui.execute_shared_ui_action(SharedUiAction::Help);
                            continue;
                        }
                        SlashParseResult::Command(SlashCommand::Status) => {
                            let _ = ui.execute_shared_ui_action(SharedUiAction::Status);
                            continue;
                        }
                        SlashParseResult::Command(SlashCommand::Mcp) => {
                            let _ = ui.execute_shared_ui_action(SharedUiAction::Mcps);
                            continue;
                        }
                        SlashParseResult::Unknown(command) => {
                            ui.emit(&UiEvent::Warning {
                                message: format!("Unknown slash command: {command}"),
                            });
                            continue;
                        }
                        SlashParseResult::NotSlash => {}
                    }

                    worker_cmd_tx
                        .send(WorkerCommand::ExecuteTurn { prompt, span })
                        .map_err(|_| {
                            LabeledError::new("Interactive worker channel closed unexpectedly")
                        })?;
                    worker_active = true;
                    break;
                }
            }

            if ui.quit_requested() {
                if worker_active {
                    cancel_requested.store(true, Ordering::SeqCst);
                    continue;
                }
                break;
            }
        }

        let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
        let _ = worker;

        Ok(Value::nothing(span))
    })
}

pub(crate) fn run_hydrated_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    messages: impl IntoIterator<Item = UiMessageSnapshot>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime + Send,
    U: InteractiveUi,
{
    ui.hydrate_transcript_from_messages(messages);
    run_interactive_loop(runtime, ui, span)
}

pub(crate) fn run_single_turn<R, U>(
    runtime: &mut R,
    ui: &mut U,
    prompt: String,
    context: Option<String>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ConversationRuntime,
    U: ProgressUi,
{
    runtime.execute_turn(ui, prompt, context, span)
}
