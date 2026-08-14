use tokio::sync::{broadcast, mpsc};

use crate::bus::{
    Bus, CompactionEvent, LlmEvent, SessionEvent, ToolEvent, TurnEvent, WarningEvent,
};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;

/// Bridges bus channels and the permission mpsc channel to the TUI via
/// [`ProgressUi::emit_batch`].
///
/// Producers publish tool/LLM/warning/compaction/turn events to the shared bus.
/// Permission request/response events still flow through the mpsc channel
/// (request/response semantics, not broadcast). This pump drains both sources,
/// reconstructs `UiEvent` values from bus events, and forwards them to the UI.
pub struct EventPump {
    /// Permission events (and other events not on the bus) via mpsc.
    permission_rx: mpsc::UnboundedReceiver<UiEvent>,
    tool_rx: broadcast::Receiver<ToolEvent>,
    llm_rx: broadcast::Receiver<LlmEvent>,
    warning_rx: broadcast::Receiver<WarningEvent>,
    compaction_rx: broadcast::Receiver<CompactionEvent>,
    turn_rx: broadcast::Receiver<TurnEvent>,
    session_rx: broadcast::Receiver<SessionEvent>,
}

impl EventPump {
    pub fn new(permission_rx: mpsc::UnboundedReceiver<UiEvent>, bus: &Bus) -> Self {
        Self {
            permission_rx,
            tool_rx: bus.tool().subscribe(),
            llm_rx: bus.llm().subscribe(),
            warning_rx: bus.warning().subscribe(),
            compaction_rx: bus.compaction().subscribe(),
            turn_rx: bus.turn().subscribe(),
            session_rx: bus.session().subscribe(),
        }
    }

    /// Collect all pending events and forward them as a batch via
    /// [`ProgressUi::emit_batch`]. Returns the number of events forwarded.
    pub fn drain_batch<U: ProgressUi>(&mut self, ui: &mut U) -> usize {
        let mut batch = Vec::new();

        // Permission events (mpsc) — pass through unchanged.
        while let Ok(event) = self.permission_rx.try_recv() {
            batch.push(event);
        }

        self.drain_tool(&mut batch);
        self.drain_llm(&mut batch);
        self.drain_warning(&mut batch);
        self.drain_compaction(&mut batch);
        self.drain_turn(&mut batch);
        self.drain_session();

        let count = batch.len();
        if !batch.is_empty() {
            log::trace!("orchestrator: forwarding {} worker events to UI", count);
            ui.emit_batch(&batch);
        }
        count
    }

    fn drain_tool(&mut self, batch: &mut Vec<UiEvent>) {
        loop {
            match self.tool_rx.try_recv() {
                Ok(ToolEvent::Start {
                    name,
                    source,
                    arguments,
                }) => batch.push(UiEvent::ToolStart {
                    name,
                    source,
                    arguments,
                }),
                Ok(ToolEvent::End {
                    name,
                    source,
                    arguments,
                    success,
                    result,
                    display,
                    error_kind,
                    message,
                }) => batch.push(UiEvent::ToolEnd {
                    name,
                    source,
                    arguments,
                    success,
                    result,
                    display,
                    error_kind,
                    message,
                }),
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    log::warn!("EventPump: skipped {n} tool events (lagged)");
                }
                Err(_) => break,
            }
        }
    }

    fn drain_llm(&mut self, batch: &mut Vec<UiEvent>) {
        loop {
            match self.llm_rx.try_recv() {
                Ok(LlmEvent::Start) => batch.push(UiEvent::LlmStart),
                Ok(LlmEvent::End {
                    response_chars,
                    tool_calls,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                }) => batch.push(UiEvent::LlmEnd {
                    response_chars,
                    tool_calls,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                }),
                Ok(LlmEvent::AssistantMessage { text }) => {
                    batch.push(UiEvent::AssistantMessage { text });
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    log::warn!("EventPump: skipped {n} llm events (lagged)");
                }
                Err(_) => break,
            }
        }
    }

    fn drain_warning(&mut self, batch: &mut Vec<UiEvent>) {
        loop {
            match self.warning_rx.try_recv() {
                Ok(WarningEvent::Message { message }) => {
                    batch.push(UiEvent::Warning { message });
                }
                Ok(WarningEvent::TurnError { message }) => {
                    batch.push(UiEvent::TurnError { message });
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    log::warn!("EventPump: skipped {n} warning events (lagged)");
                }
                Err(_) => break,
            }
        }
    }

    fn drain_compaction(&mut self, batch: &mut Vec<UiEvent>) {
        loop {
            match self.compaction_rx.try_recv() {
                Ok(CompactionEvent::Started { source }) => {
                    batch.push(UiEvent::CompactionStarted {
                        source: source.unwrap_or_default(),
                    });
                }
                Ok(CompactionEvent::SummaryChunk {
                    source,
                    delta,
                    aggregated,
                }) => batch.push(UiEvent::CompactionSummaryChunk {
                    source,
                    delta,
                    aggregated,
                }),
                Ok(CompactionEvent::Triggered {
                    source,
                    summarized_count,
                    kept_recent_count,
                    summary_preview,
                    summary_body,
                }) => batch.push(UiEvent::CompactionTriggered {
                    source,
                    summarized_count,
                    kept_recent_count,
                    summary_preview,
                    summary_body,
                }),
                Ok(CompactionEvent::Failed { source, message }) => {
                    batch.push(UiEvent::CompactionFailed { source, message });
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    log::warn!("EventPump: skipped {n} compaction events (lagged)");
                }
                Err(_) => break,
            }
        }
    }

    fn drain_turn(&mut self, batch: &mut Vec<UiEvent>) {
        loop {
            match self.turn_rx.try_recv() {
                Ok(TurnEvent::Started { prompt, task_id }) => {
                    log::trace!(
                        "EventPump: turn started prompt_len={} task_id={task_id:?}",
                        prompt.len()
                    );
                }
                Ok(TurnEvent::TurnCompleted { tool_calls }) => {
                    batch.push(UiEvent::Completed { tool_calls });
                }
                // A2A-only completion — not for the TUI.
                Ok(TurnEvent::TaskCompleted { .. }) => {}
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    log::warn!("EventPump: skipped {n} turn events (lagged)");
                }
                Err(_) => break,
            }
        }
    }

    fn drain_session(&mut self) {
        loop {
            match self.session_rx.try_recv() {
                Ok(SessionEvent::Started {
                    session_id,
                    hydrated,
                }) => {
                    log::trace!("EventPump: session started {session_id} hydrated={hydrated}");
                }
                Ok(SessionEvent::Ended { session_id }) => {
                    log::trace!("EventPump: session ended {session_id}");
                }
                Ok(SessionEvent::Switched {
                    from_session_id,
                    to_session_id,
                }) => {
                    log::trace!(
                        "EventPump: session switched from {from_session_id:?} to {to_session_id}"
                    );
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    log::warn!("EventPump: skipped {n} session events (lagged)");
                }
                Err(_) => break,
            }
        }
    }
}
