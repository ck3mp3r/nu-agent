//! Application state types and the `AppState` reducer root.
//!
//! The root keeps the state value and the orchestration that spans more than
//! one domain; each topical concern the root used to inline lives in its own
//! submodule.

// region:    --- Modules

mod lifecycle;
mod support;
mod tool_dispatch;
mod types;
mod windowing;
mod yank;

pub use types::{
    AbortState, CommandPaletteAction, CompactionLine, CompactionStatus, DiscoverableSkill,
    ImmediateSubmission, InfoPanel, McpServerState, McpServerUsabilityState, McpToggleRequest,
    PromptFacts, PromptStatus, QueuedPrompt, ToolCallLine, ToolCallStatus, TranscriptRole, UiPhase,
};

pub(crate) use support::QUIT_CONFIRM_WINDOW;

// endregion: --- Modules

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use agnt_runtime::orchestrator::events::{
    OrchestratorEvent, PickerReply, RefreshAgentPickerResult, RefreshSessionPickerResult, UiRequest,
};
use agnt_runtime::protocol::contracts::SharedUiAction;

use super::compaction::CompactionState;
use super::input::InputState;
use super::llm::LlmState;
use super::permission::PermissionState;
use super::picker::{PickerContainer, SwitchRequest};
use super::scroll::ScrollState;
use super::status::StatusState;
use super::tool::ToolState;
use super::transcript_store::TranscriptStore;
use super::turn::TurnState;
use crate::interaction::cancel::CancelController;
use crate::interaction::input::{TerminalEvent, map_terminal_event};
use crate::rendering::theme::{ThemeName, TuiTheme};

#[derive(Debug)]
pub struct AppState {
    pub phase: UiPhase,
    pub input_locked: bool,
    pub abort: AbortState,
    pub transcript: TranscriptStore,
    pub input: InputState,
    pub scroll: ScrollState,
    pub quit_requested: bool,
    /// When the Ctrl-C quit confirmation was armed, if it is armed.
    pub quit_confirm_since: Option<Instant>,
    /// How long an armed quit confirmation stays valid.
    pub quit_confirm_window: Duration,
    pub picker: PickerContainer,
    pub(crate) pending_switch_requests: VecDeque<SwitchRequest>,
    pub(crate) pending_launch: VecDeque<SharedUiAction>,
    pub(crate) pending_theme_persist: VecDeque<String>,
    /// The outbound events the orchestrator has not taken yet, in production
    /// order. An event leaves this queue only after a successful send, so a
    /// busy or wedged consumer delays the delivery instead of losing it or
    /// stalling the loop.
    pub(crate) outbound: OutboundQueue,
    pub(crate) pending_session_picker_refresh: bool,
    pub(crate) pending_agent_picker_refresh: bool,
    /// The surface's own reply hands: one mpsc pair per picker kind. The
    /// catalog is surface state, so it comes back here, never on a bus
    /// channel.
    pub(crate) session_picker_tx: PickerReply<RefreshSessionPickerResult>,
    pub(crate) session_picker_rx: tokio::sync::mpsc::Receiver<RefreshSessionPickerResult>,
    pub(crate) agent_picker_tx: PickerReply<RefreshAgentPickerResult>,
    pub(crate) agent_picker_rx: tokio::sync::mpsc::Receiver<RefreshAgentPickerResult>,
    pub info_panel: Option<InfoPanel>,
    pub info_panel_scroll: usize,
    /// Whether the render-stats popup is showing. `/stats` toggles it.
    pub(crate) render_stats_visible: bool,
    pub permission: PermissionState,
    pub status: StatusState,
    pub(crate) prompt_items: Vec<QueuedPrompt>,
    pub(crate) pending_prompt_ids: VecDeque<u64>,
    pub(crate) pending_immediate_submissions: VecDeque<ImmediateSubmission>,
    pub(crate) active_prompt_id: Option<u64>,
    pub(crate) next_prompt_id: u64,
    pub(crate) active_cycle: bool,
    pub tool: ToolState,
    pub llm: LlmState,
    pub compaction: CompactionState,
    pub turn: TurnState,
    pub theme: TuiTheme,
    pub(crate) theme_name: ThemeName,
    pub event_tx: tokio::sync::mpsc::Sender<agnt_runtime::orchestrator::events::OrchestratorEvent>,
}

impl Default for AppState {
    fn default() -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<
            agnt_runtime::orchestrator::events::OrchestratorEvent,
        >(256);
        let (session_picker_tx, session_picker_rx) =
            tokio::sync::mpsc::channel(super::picker::PICKER_REPLY_CAPACITY);
        let (agent_picker_tx, agent_picker_rx) =
            tokio::sync::mpsc::channel(super::picker::PICKER_REPLY_CAPACITY);
        Self {
            phase: UiPhase::Idle,
            input_locked: false,
            abort: AbortState::default(),
            transcript: TranscriptStore::default(),
            input: InputState::default(),
            scroll: ScrollState::default(),
            quit_requested: false,
            quit_confirm_since: None,
            quit_confirm_window: QUIT_CONFIRM_WINDOW,
            picker: PickerContainer::default(),
            pending_switch_requests: VecDeque::new(),
            pending_launch: VecDeque::new(),
            pending_theme_persist: VecDeque::new(),
            outbound: OutboundQueue::default(),
            pending_session_picker_refresh: false,
            pending_agent_picker_refresh: false,
            session_picker_tx,
            session_picker_rx,
            agent_picker_tx,
            agent_picker_rx,
            info_panel: None,
            info_panel_scroll: 0,
            render_stats_visible: false,
            permission: PermissionState::default(),
            status: StatusState::default(),
            prompt_items: Vec::new(),
            pending_prompt_ids: VecDeque::new(),
            pending_immediate_submissions: VecDeque::new(),
            active_prompt_id: None,
            next_prompt_id: 1,
            active_cycle: false,
            tool: ToolState::default(),
            llm: LlmState,
            compaction: CompactionState::default(),
            turn: TurnState,
            theme: TuiTheme::default(),
            theme_name: ThemeName::default(),
            event_tx,
        }
    }
}

/// What one attempt to hand the front outbound event to the orchestrator did.
///
/// `Full` is transient — the consumer is alive but busy, so the event keeps its
/// place. `Closed` is terminal: the consumer is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutboundDelivery {
    Sent,
    Empty,
    Full,
    Closed,
}

/// The outbound events awaiting delivery, in production order.
///
/// `OrchestratorEvent` carries no `Debug` (its picker hands hold channels), so a
/// debug dump reports the queue's depth instead of its content.
#[derive(Default)]
pub(crate) struct OutboundQueue(VecDeque<OrchestratorEvent>);

impl std::fmt::Debug for OutboundQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OutboundQueue").field(&self.0.len()).finish()
    }
}

impl AppState {
    /// Drain the state's pending effects onto the persistent outbound queue.
    ///
    /// The production gates stay where they are: a prompt still stacks while a
    /// turn runs, and a decision still needs its open card. This call only moves
    /// what each gate released, in production order.
    pub fn enqueue_pending_events(&mut self, cancel_controller: &CancelController) {
        let mut events = Vec::new();
        while let Some(prompt) = self.take_next_prompt_for_execution() {
            // A surface submit carries no task attribution: the loop owns every
            // external intake, so its own queue dispatches those turns.
            events.push(OrchestratorEvent::PromptSubmitted { text: prompt });
        }
        while let Some(decision) = self.permission.take_next_submission() {
            events.push(OrchestratorEvent::PermissionDecision { decision });
        }
        while let Some(req) = self.take_next_switch_request() {
            match req {
                SwitchRequest::Model(spec) => {
                    events.push(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                        spec,
                    }));
                }
                SwitchRequest::Agent(name) => {
                    events.push(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                        name,
                    }));
                }
                SwitchRequest::Session(id) => {
                    events.push(OrchestratorEvent::UiRequest(UiRequest::SwitchSession {
                        id,
                    }));
                }
                SwitchRequest::Theme(name) => {
                    if let Some(theme_name) = ThemeName::from_name(&name) {
                        self.theme = theme_name.resolve();
                        self.theme_name = theme_name;
                        self.pending_theme_persist.push_back(name);
                    }
                }
            }
        }
        while let Some(req) = self.status.mcp.take_next_mcp_toggle_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::ToggleMcp {
                server: req.server_name,
                enable: req.enable,
            }));
        }
        if let Some(reply) = self.take_next_session_picker_refresh_request() {
            events.push(OrchestratorEvent::UiRequest(
                UiRequest::RefreshSessionPicker { reply: Some(reply) },
            ));
        }
        if let Some(reply) = self.take_next_agent_picker_refresh_request() {
            events.push(OrchestratorEvent::UiRequest(
                UiRequest::RefreshAgentPicker { reply: Some(reply) },
            ));
        }
        if cancel_controller.take_cancel_requested() {
            events.push(OrchestratorEvent::CancelRequested);
        }
        self.outbound.0.extend(events);
    }

    /// Try to hand the front outbound event to the orchestrator.
    ///
    /// The event leaves the queue only after a successful send, so a full
    /// channel keeps it first (FIFO, exactly-once) and a closed one reports the
    /// consumer gone. Nothing here waits.
    pub(crate) fn try_deliver_outbound(
        &mut self,
        event_tx: &tokio::sync::mpsc::Sender<OrchestratorEvent>,
    ) -> OutboundDelivery {
        let Some(event) = self.outbound.0.pop_front() else {
            return OutboundDelivery::Empty;
        };
        match event_tx.try_send(event) {
            Ok(()) => OutboundDelivery::Sent,
            Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                // The send handed the event back: it keeps its place at the head.
                self.outbound.0.push_front(event);
                OutboundDelivery::Full
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => OutboundDelivery::Closed,
        }
    }

    /// Drop the effects a replayed log produced: a hydration must not deliver
    /// the recorded prompts and decisions, and must not keep them either.
    pub(crate) fn discard_pending_events(&mut self, cancel_controller: &CancelController) {
        self.enqueue_pending_events(cancel_controller);
        self.outbound.0.clear();
    }

    /// Prompts that have completed execution, oldest first — the input
    /// history source for [`InputState::history_up`] and
    /// [`InputState::history_down`].
    pub(crate) fn submitted_prompt_texts(&self) -> Vec<String> {
        self.prompt_items
            .iter()
            .filter(|p| p.status == PromptStatus::Done)
            .map(|p| p.prompt_text.clone())
            .collect()
    }

    pub fn reduce_terminal_event(
        &mut self,
        event: &TerminalEvent,
        cancel_controller: Option<&CancelController>,
    ) -> bool {
        // The terminal layer reports keys, wheel notches, and resizes; the
        // reducer owns what they mean (the input lock, the open card, the
        // keymap).
        let Some(input) = map_terminal_event(event) else {
            return false;
        };
        let folded = crate::interaction::reducer::reduce(self, input, cancel_controller);
        // A fold that queued a shared UI action cannot apply it inline: the key
        // lens is still borrowing the state it dispatched from.
        folded | self.drain_launch_requests()
    }

    /// Shift tool-call `block_index` bookkeeping after transcript-cap
    /// eviction: tool calls store indices into the transcript entry list, and
    /// eviction moves every surviving entry down by the evicted count. An index
    /// below the evicted range points at a removed entry and becomes `None`.
    /// Streaming cursors are shifted by the store itself.
    pub fn shift_bookkeeping_after_eviction(&mut self, evicted_count: usize) {
        if evicted_count == 0 {
            return;
        }
        let shift = |n: Option<usize>| {
            n.and_then(|idx| {
                if idx >= evicted_count {
                    Some(idx - evicted_count)
                } else {
                    None
                }
            })
        };
        for call in &mut self.tool.calls {
            call.block_index = shift(call.block_index);
        }
    }
}
