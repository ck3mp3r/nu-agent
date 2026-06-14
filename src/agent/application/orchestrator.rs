use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use nu_protocol::{LabeledError, Span, Value};

use crate::agent::{
    application::{
        command::{
            pending::PendingOps,
            poll::{PollOutcome, poll_pending},
            pump::EventPump,
            router::CommandRouter,
        },
        turn_outcome::TurnOutcome,
    },
    protocol::{
        compaction::CompactionTriggerSource,
        contracts::{
            CoreRuntime, ExtendedRuntime, InteractiveUi, McpToggleRequest, McpUsabilityState,
            ProgressUi, SharedUiAction, UiMessageSnapshot,
        },
        event::UiEvent,
        permission::submit_active_permission_decision,
        slash::{SlashCommand, SlashParseResult, parse_slash_command},
    },
};

pub(crate) type McpToggleResult = (
    Result<McpUsabilityState, String>,
    usize,
    usize,
    Vec<(String, Vec<String>)>,
);
type PendingMcpToggle = (String, mpsc::Receiver<McpToggleResult>);
pub(crate) type ModelSwitchResult = Result<String, String>;
pub(crate) type AgentSwitchResult = Result<(String, String), String>;
type PendingModelSwitch = mpsc::Receiver<ModelSwitchResult>;
type PendingAgentSwitch = mpsc::Receiver<AgentSwitchResult>;
type PendingAutoCompaction = mpsc::Receiver<Option<String>>;
type PendingCompactionTrigger = mpsc::Receiver<Option<String>>;

pub(crate) enum WorkerCommand {
    ExecuteTurn {
        prompt: String,
        span: Span,
    },
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
    SwitchModel {
        model_spec: String,
        response_tx: mpsc::Sender<ModelSwitchResult>,
    },
    SwitchAgent {
        agent_name: String,
        response_tx: mpsc::Sender<AgentSwitchResult>,
    },
    ClearSession,
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
}

pub(crate) fn run_interactive_loop<R, U>(
    runtime: &mut R,
    ui: &mut U,
    mailbox_rx: Option<std::sync::mpsc::Receiver<crate::agent::mailbox::IncomingMessage>>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ExtendedRuntime + Send,
    U: InteractiveUi,
{
    let mut last_authoritative_visible_count = runtime.llm_visible_mcp_tool_count();
    ui.set_active_model_identity(runtime.active_model_identity().as_str());
    for (server_name, names) in runtime.llm_visible_mcp_tool_names_by_server() {
        ui.set_mcp_visible_tool_count_by_server_name(&server_name, names.len());
        ui.set_mcp_visible_tool_names_by_server_name(&server_name, names);
    }

    std::thread::scope(|scope| {
        let (worker_cmd_tx, worker_cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (worker_event_tx, worker_event_rx) = mpsc::channel::<UiEvent>();
        let (worker_result_tx, worker_result_rx) = mpsc::channel::<TurnOutcome>();
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel_requested);

        let worker = scope.spawn(move || {
            let mut worker_ui = WorkerProgressUi {
                events: worker_event_tx,
                cancel_requested: worker_cancel,
            };
            let mut router = CommandRouter::new(runtime);

            while let Ok(command) = worker_cmd_rx.recv() {
                if !router.dispatch(command, &mut worker_ui, &worker_result_tx) {
                    break;
                }
            }
        });

        let mut event_pump = EventPump::new(worker_event_rx);
        let mut pending_ops = PendingOps::new();
        let mut worker_active = false;
        let mut pending_mcp_toggles: Vec<PendingMcpToggle> = Vec::new();
        let mut pending_auto_compaction: Option<PendingAutoCompaction> = None;
        let mut pending_compaction_trigger: Option<PendingCompactionTrigger> = None;
        let mut pending_model_switch: Option<PendingModelSwitch> = None;
        let mut pending_agent_switch: Option<PendingAgentSwitch> = None;
        let mut pending_mailbox_prompts: Vec<String> = Vec::new();

        loop {
            ui.pump_once();

            while let Some(McpToggleRequest {
                server_name,
                enable,
            }) = ui.take_next_mcp_toggle_request()
            {
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

            if let Some(response_rx) = pending_model_switch.take() {
                match poll_pending(response_rx) {
                    PollOutcome::Ready(Ok(active_identity)) => {
                        ui.set_active_model_identity(active_identity.as_str());
                        ui.emit(&UiEvent::Warning {
                            message: format!("Model switched: {active_identity}"),
                        });
                    }
                    PollOutcome::Ready(Err(message)) => {
                        ui.emit(&UiEvent::Warning { message });
                    }
                    PollOutcome::Pending(rx) => pending_model_switch = Some(rx),
                    PollOutcome::Disconnected => {
                        ui.emit(&UiEvent::Warning {
                            message: "Model switch worker disconnected".to_string(),
                        });
                    }
                }
            }

            if let Some(response_rx) = pending_agent_switch.take() {
                match poll_pending(response_rx) {
                    PollOutcome::Ready(Ok((agent_identity, model_identity))) => {
                        ui.set_active_agent_identity(&agent_identity);
                        ui.set_active_model_identity(&model_identity);
                        ui.emit(&UiEvent::Warning {
                            message: format!("Agent switched to: {agent_identity}"),
                        });
                    }
                    PollOutcome::Ready(Err(message)) => {
                        ui.emit(&UiEvent::Warning { message });
                    }
                    PollOutcome::Pending(rx) => pending_agent_switch = Some(rx),
                    PollOutcome::Disconnected => {
                        ui.emit(&UiEvent::Warning {
                            message: "Agent switch worker channel closed".to_string(),
                        });
                    }
                }
            }

            let mut retained = Vec::new();
            for (server_name, response_rx) in pending_mcp_toggles.drain(..) {
                match response_rx.try_recv() {
                    Ok((
                        Ok(state),
                        visible_count,
                        visible_count_for_server,
                        visible_names_by_server,
                    )) => {
                        last_authoritative_visible_count = visible_count;
                        ui.set_mcp_visible_tool_count_by_server_name(
                            &server_name,
                            visible_count_for_server,
                        );
                        for (server, names) in visible_names_by_server {
                            ui.set_mcp_visible_tool_names_by_server_name(&server, names);
                        }
                        ui.set_mcp_server_state_with_details(
                            &server_name,
                            state,
                            None,
                            visible_count,
                        )
                    }
                    Ok((
                        Err(err),
                        visible_count,
                        visible_count_for_server,
                        visible_names_by_server,
                    )) => {
                        last_authoritative_visible_count = visible_count;
                        ui.set_mcp_visible_tool_count_by_server_name(
                            &server_name,
                            visible_count_for_server,
                        );
                        for (server, names) in visible_names_by_server {
                            ui.set_mcp_visible_tool_names_by_server_name(&server, names);
                        }
                        ui.set_mcp_server_state_with_details(
                            &server_name,
                            McpUsabilityState::Failed,
                            Some(err),
                            visible_count,
                        )
                    }
                    Err(mpsc::TryRecvError::Empty) => retained.push((server_name, response_rx)),
                    Err(mpsc::TryRecvError::Disconnected) => ui.set_mcp_server_state_with_details(
                        &server_name,
                        McpUsabilityState::Failed,
                        Some("toggle worker disconnected".to_string()),
                        last_authoritative_visible_count,
                    ),
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

            event_pump.drain_batch(ui);

            while let Ok(outcome) = worker_result_rx.try_recv() {
                worker_active = false;
                match outcome {
                    TurnOutcome::Success(_) => {}
                    TurnOutcome::Cancelled => {
                        // Silently ignore cancellation
                    }
                    TurnOutcome::Error(error) => {
                        // Display error inline as a turn error
                        ui.emit(&UiEvent::TurnError {
                            message: format!("Turn failed: {}", error.msg),
                        });
                    }
                }
            }

            if let Some(error) = ui.fatal_error() {
                let _ = worker_cmd_tx.send(WorkerCommand::Shutdown);
                return Err(LabeledError::new(format!("Interactive UI failed: {error}")));
            }

            while ui.take_next_model_picker_launch_request() {
                let _ = ui.execute_shared_ui_action(SharedUiAction::Models);
            }

            while ui.take_next_agent_picker_launch_request() {
                let _ = ui.execute_shared_ui_action(SharedUiAction::Agents);
            }

            while let Some(model_spec) = ui.take_next_model_switch_request() {
                if worker_active {
                    pending_ops.queue_model_switch(model_spec.clone());
                    ui.emit(&UiEvent::Warning {
                        message: format!("Model switch queued for next turn: {model_spec}"),
                    });
                } else if pending_model_switch.is_none() {
                    let (response_tx, response_rx) = mpsc::channel();
                    if worker_cmd_tx
                        .send(WorkerCommand::SwitchModel {
                            model_spec,
                            response_tx,
                        })
                        .is_ok()
                    {
                        pending_model_switch = Some(response_rx);
                    } else {
                        ui.emit(&UiEvent::Warning {
                            message: "Model switch worker channel closed".to_string(),
                        });
                    }
                } else {
                    pending_ops.queue_model_switch(model_spec);
                }
            }

            while let Some(agent_name) = ui.take_next_agent_switch_request() {
                if worker_active {
                    pending_ops.queue_agent_switch(agent_name.clone());
                    ui.emit(&UiEvent::Warning {
                        message: format!("Agent switch queued for next turn: {agent_name}"),
                    });
                } else if pending_agent_switch.is_none() {
                    let (response_tx, response_rx) = mpsc::channel();
                    if worker_cmd_tx
                        .send(WorkerCommand::SwitchAgent {
                            agent_name,
                            response_tx,
                        })
                        .is_ok()
                    {
                        pending_agent_switch = Some(response_rx);
                    } else {
                        ui.emit(&UiEvent::Warning {
                            message: "Agent switch worker channel closed".to_string(),
                        });
                    }
                } else {
                    pending_ops.queue_agent_switch(agent_name);
                }
            }

            while let Some(submission) = ui.take_next_permission_decision_submission() {
                match submit_active_permission_decision(
                    submission.request_id.clone(),
                    submission.decision,
                    submission.matched_rule_identity,
                ) {
                    crate::agent::protocol::permission::SubmitOutcome::Accepted => {}
                    crate::agent::protocol::permission::SubmitOutcome::Ignored { reason } => {
                        ui.emit(&UiEvent::PermissionDecisionIgnored {
                            request_id: submission.request_id,
                            reason: reason.to_string(),
                        });
                    }
                }
            }

            if !worker_active
                && pending_model_switch.is_none()
                && let Some(model_spec) = pending_ops.take_queued_model_switch()
            {
                let (response_tx, response_rx) = mpsc::channel();
                if worker_cmd_tx
                    .send(WorkerCommand::SwitchModel {
                        model_spec,
                        response_tx,
                    })
                    .is_ok()
                {
                    pending_model_switch = Some(response_rx);
                } else {
                    ui.emit(&UiEvent::Warning {
                        message: "Model switch worker channel closed".to_string(),
                    });
                }
            }

            if !worker_active
                && pending_agent_switch.is_none()
                && let Some(agent_name) = pending_ops.take_queued_agent_switch()
            {
                let (response_tx, response_rx) = mpsc::channel();
                if worker_cmd_tx
                    .send(WorkerCommand::SwitchAgent {
                        agent_name,
                        response_tx,
                    })
                    .is_ok()
                {
                    pending_agent_switch = Some(response_rx);
                } else {
                    ui.emit(&UiEvent::Warning {
                        message: "Agent switch worker channel closed".to_string(),
                    });
                }
            }

            if !worker_active && pending_model_switch.is_none() {
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
                        SlashParseResult::Command(SlashCommand::Models) => {
                            let _ = ui.execute_shared_ui_action(SharedUiAction::Models);
                            continue;
                        }
                        SlashParseResult::Command(SlashCommand::Agent) => {
                            let _ = ui.execute_shared_ui_action(SharedUiAction::Agents);
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

            // Poll mailbox for incoming messages
            if let Some(ref rx) = mailbox_rx {
                while let Ok(msg) = rx.try_recv() {
                    if msg.message == "/clear" {
                        let _ = worker_cmd_tx.send(WorkerCommand::ClearSession);
                        ui.clear_transcript();
                        continue;
                    }
                    let prompt = match msg.kind.as_str() {
                        "task" => format!("[TASK from: {}] {}", msg.from, msg.message),
                        "completion" => {
                            format!("[COMPLETED from: {}] {}", msg.from, msg.message)
                        }
                        "question" => format!(
                            "[QUESTION from: {} — BLOCKED, needs your decision] {}",
                            msg.from, msg.message
                        ),
                        _ => format!("[from: {}] {}", msg.from, msg.message),
                    };
                    if !worker_active {
                        ui.display_incoming_message(&prompt);
                        worker_cmd_tx
                            .send(WorkerCommand::ExecuteTurn { prompt, span })
                            .map_err(|_| LabeledError::new("Worker channel closed"))?;
                        worker_active = true;
                        break;
                    } else {
                        pending_mailbox_prompts.push(prompt);
                    }
                }
            }

            // Drain pending mailbox prompts when worker becomes idle
            if !worker_active
                && !pending_mailbox_prompts.is_empty()
                && let Some(prompt) = pending_mailbox_prompts.drain(0..1).next()
            {
                ui.display_incoming_message(&prompt);
                worker_cmd_tx
                    .send(WorkerCommand::ExecuteTurn { prompt, span })
                    .map_err(|_| LabeledError::new("Worker channel closed"))?;
                worker_active = true;
            }

            if ui.quit_requested() {
                if worker_active {
                    cancel_requested.store(true, Ordering::SeqCst);
                    continue;
                }
                if pending_model_switch.is_some()
                    || pending_agent_switch.is_some()
                    || !pending_mcp_toggles.is_empty()
                    || pending_auto_compaction.is_some()
                    || pending_compaction_trigger.is_some()
                {
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
    last_total_tokens: Option<u64>,
    mailbox_rx: Option<std::sync::mpsc::Receiver<crate::agent::mailbox::IncomingMessage>>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: ExtendedRuntime + Send,
    U: InteractiveUi,
{
    ui.hydrate_transcript_from_messages(messages, last_total_tokens);
    run_interactive_loop(runtime, ui, mailbox_rx, span)
}

pub(crate) fn run_single_turn<R, U>(
    runtime: &mut R,
    ui: &mut U,
    prompt: String,
    context: Option<String>,
    span: Span,
) -> Result<Value, LabeledError>
where
    R: CoreRuntime,
    U: ProgressUi,
{
    runtime.execute_turn(ui, prompt, context, span)
}
