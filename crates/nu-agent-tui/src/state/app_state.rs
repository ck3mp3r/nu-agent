//! Application state types and the `AppState` reducer root.

use std::collections::VecDeque;

use nu_agent_core::orchestrator::{OrchestratorEvent, UiRequest, UiStateEvent};
use nu_agent_core::protocol::contracts::SharedUiAction;
use nu_agent_core::transcript::items::{TranscriptEntry, TranscriptEntryKind};

use super::compaction::CompactionState;
use super::input::InputState;
use super::llm::LlmState;
use super::permission::PermissionState;
use super::picker::{ActivePicker, PickerContainer, PickerOption, PickerPayload, SwitchRequest};
use super::scroll::ScrollState;
use super::status::StatusState;
use super::tool::ToolState;
use super::transcript_store::TranscriptStore;
use super::turn::TurnState;
use crate::interaction::cancel::CancelController;
use crate::interaction::dispatch::rewrite_action;
use crate::interaction::input::{TerminalEvent, map_terminal_event};
use crate::interaction::reducer::{ReducerInput, reduce_with_cancel_controller};
use crate::rendering::theme::{ThemeName, TuiTheme};

const STARTUP_LOGOS: &[&str] = &[
    include_str!("../logos/00.txt"),
    include_str!("../logos/01.txt"),
    include_str!("../logos/02.txt"),
    include_str!("../logos/03.txt"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryVisualInfo {
    pub start_visual_row: usize,
    pub visual_row_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPhase {
    Idle,
    Busy,
    AbortPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Assistant,
    System,
    Compaction,
    Tool,
    ToolDisplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStatus {
    Queued,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    InProgress,
    Done,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStatus {
    InProgress,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionLine {
    pub source: String,
    pub status: CompactionStatus,
    pub entry_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPaletteAction {
    Compact,
    Help,
    Status,
    Mcps,
    Skills,
    Models,
    Agents,
    Sessions,
    Theme,
}

impl CommandPaletteAction {
    pub const PALETTE_ACTIONS: &[CommandPaletteAction] = &[
        Self::Help,
        Self::Status,
        Self::Mcps,
        Self::Skills,
        Self::Models,
        Self::Agents,
        Self::Sessions,
        Self::Theme,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "/compact",
            Self::Help => "Help",
            Self::Status => "Status",
            Self::Mcps => "MCPs",
            Self::Skills => "Skills",
            Self::Models => "Models",
            Self::Agents => "Agents",
            Self::Sessions => "Sessions",
            Self::Theme => "Theme",
        }
    }

    pub fn summary(&self) -> &'static str {
        match self {
            Self::Compact => "Run /compact now",
            Self::Help => "View key help",
            Self::Status => "View runtime status",
            Self::Mcps => "Manage MCP servers",
            Self::Skills => "List available skills",
            Self::Models => "Open model picker",
            Self::Agents => "Open agent picker",
            Self::Sessions => "Switch to an existing session",
            Self::Theme => "Open theme picker",
        }
    }

    pub fn info_panel(&self) -> Option<InfoPanel> {
        match self {
            Self::Compact => None,
            Self::Help => Some(InfoPanel::Help),
            Self::Status => Some(InfoPanel::Status),
            Self::Mcps => Some(InfoPanel::Mcps),
            Self::Skills => Some(InfoPanel::Skills),
            Self::Models => None,
            Self::Agents => None,
            Self::Sessions => None,
            Self::Theme => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoPanel {
    Help,
    Status,
    Mcps,
    Skills,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverableSkill {
    pub source_priority: u8,
    pub source: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerState {
    pub name: String,
    pub state: McpServerUsabilityState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToggleRequest {
    pub server_name: String,
    pub enable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerUsabilityState {
    Enabled,
    Disabled,
    Failed,
}

impl McpServerUsabilityState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Failed => "failed",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Enabled => "🟢",
            Self::Disabled => "⚪",
            Self::Failed => "🔴",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub id: u64,
    pub prompt_text: String,
    pub status: PromptStatus,
    pub entry_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallLine {
    pub id: u64,
    pub status: ToolCallStatus,
    pub key: String,
    pub entry_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbortState {
    pub pending: bool,
    pub confirmation_marker: u64,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub phase: UiPhase,
    pub input_locked: bool,
    pub abort: AbortState,
    pub transcript: TranscriptStore,
    pub input: InputState,
    pub scroll: ScrollState,
    pub quit_requested: bool,
    pub picker: PickerContainer,
    pub(crate) pending_switch_requests: VecDeque<SwitchRequest>,
    pub(crate) pending_launch: VecDeque<SharedUiAction>,
    pub info_panel: Option<InfoPanel>,
    pub info_panel_scroll: usize,
    pub permission: PermissionState,
    pub status: StatusState,
    pub(crate) prompt_items: Vec<QueuedPrompt>,
    pub(crate) pending_prompt_ids: VecDeque<u64>,
    pub(crate) pending_immediate_submissions: VecDeque<String>,
    pub(crate) active_prompt_id: Option<u64>,
    pub(crate) next_prompt_id: u64,
    pub(crate) active_cycle: bool,
    pub tool: ToolState,
    pub llm: LlmState,
    pub compaction: CompactionState,
    pub turn: TurnState,
    pub theme: TuiTheme,
    pub event_tx: tokio::sync::mpsc::Sender<nu_agent_core::orchestrator::OrchestratorEvent>,
}

impl Default for AppState {
    fn default() -> Self {
        let (event_tx, _event_rx) =
            tokio::sync::mpsc::channel::<nu_agent_core::orchestrator::OrchestratorEvent>(256);
        Self {
            phase: UiPhase::Idle,
            input_locked: false,
            abort: AbortState::default(),
            transcript: TranscriptStore::default(),
            input: InputState::default(),
            scroll: ScrollState::default(),
            quit_requested: false,
            picker: PickerContainer::default(),
            pending_switch_requests: VecDeque::new(),
            pending_launch: VecDeque::new(),
            info_panel: None,
            info_panel_scroll: 0,
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
            event_tx,
        }
    }
}

impl AppState {
    pub fn take_pending_events(
        &mut self,
        cancel_controller: &CancelController,
    ) -> Vec<OrchestratorEvent> {
        let mut events = Vec::new();
        while let Some(prompt) = self.take_next_prompt_for_execution() {
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
                        self.transcript.clear_assistant_projection_cache();
                        self.transcript.visual_info_dirty = true;
                    }
                }
            }
        }
        while let Some(req) = self.status.take_next_mcp_toggle_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::ToggleMcp {
                server: req.server_name,
                enable: req.enable,
            }));
        }
        if cancel_controller.take_cancel_requested() {
            events.push(OrchestratorEvent::CancelRequested);
        }
        events
    }

    pub fn take_pending_ui_state_events(&mut self) -> Vec<UiStateEvent> {
        let mut events = Vec::new();
        while let Some(action) = self.take_next_launch_request() {
            events.push(UiStateEvent::ExecuteSharedUiAction(action));
        }
        events
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
        let Some(mapped_action) = map_terminal_event(event, self.input_locked) else {
            return false;
        };
        let (action, force_changed) = rewrite_action(self, mapped_action);
        let changed =
            reduce_with_cancel_controller(self, ReducerInput::User(action), cancel_controller);
        force_changed || changed
    }

    pub fn reduce_ui_state_event(&mut self, event: UiStateEvent) {
        match event {
            UiStateEvent::SetActiveModelIdentity(_)
            | UiStateEvent::SetActivePersonaIcon(_)
            | UiStateEvent::SetContextWindowMaxTokens(_)
            | UiStateEvent::SetMcpServerState { .. }
            | UiStateEvent::SetMcpVisibleToolCount { .. }
            | UiStateEvent::SetMcpVisibleToolNames { .. } => {}
            UiStateEvent::SetActiveAgentIdentity(s) => self.set_active_agent_identity(&s),
            UiStateEvent::ClearTranscript => self.clear_transcript(),
            UiStateEvent::HydrateTranscript {
                messages,
                last_total_tokens,
            } => {
                self.transcript.hydrate_from_messages(
                    messages,
                    last_total_tokens,
                    &mut self.status,
                    &mut self.tool,
                    &mut self.compaction,
                );
            }
            UiStateEvent::SetSessionPickerOptions(sessions) => {
                let tui_options: Vec<PickerOption> = sessions
                    .into_iter()
                    .map(|info| {
                        let title = info.title.clone();
                        let display = title.clone().unwrap_or_else(|| "(untitled)".to_string());
                        PickerOption {
                            id: info.id.clone(),
                            display: display.clone(),
                            search_text: display.clone(),
                            payload: PickerPayload::Session {
                                session_id: info.id,
                                title,
                                created_at: info.last_active,
                            },
                        }
                    })
                    .collect();
                self.set_picker_options(ActivePicker::Session, tui_options)
            }
            UiStateEvent::DisplayIncomingMessage(msg) => self.display_incoming_message(&msg),
            UiStateEvent::ExecuteSharedUiAction(action) => {
                self.execute_shared_ui_action(action);
            }
            UiStateEvent::PushStartupLogo => {
                self.push_startup_logo();
            }
        }
    }

    fn display_incoming_message(&mut self, text: &str) {
        self.enqueue_external_prompt(text.to_string());
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        match action {
            SharedUiAction::Help => {
                self.open_info_panel(InfoPanel::Help);
                true
            }
            SharedUiAction::Status => {
                self.open_info_panel(InfoPanel::Status);
                true
            }
            SharedUiAction::Mcps => {
                self.open_info_panel(InfoPanel::Mcps);
                true
            }
            SharedUiAction::Models => {
                self.info_panel = None;
                self.picker.open(ActivePicker::Model);
                self.ensure_invariants();
                true
            }
            SharedUiAction::Agents => {
                self.close_info_panel();
                self.picker.open(ActivePicker::Agent);
                self.ensure_invariants();
                true
            }
            SharedUiAction::Sessions => {
                self.close_info_panel();
                self.picker.open(ActivePicker::Session);
                self.ensure_invariants();
                true
            }
            SharedUiAction::Themes => {
                let options = ThemeName::all()
                    .into_iter()
                    .map(|name| {
                        let display_name = match name {
                            ThemeName::CatppuccinMocha => "Catppuccin Mocha".to_string(),
                            ThemeName::CatppuccinLatte => "Catppuccin Latte".to_string(),
                        };
                        PickerOption {
                            id: format!("{name:?}"),
                            display: display_name.clone(),
                            search_text: display_name.clone(),
                            payload: PickerPayload::Theme,
                        }
                    })
                    .collect();
                self.set_picker_options(ActivePicker::Theme, options);
                self.close_info_panel();
                self.picker.open(ActivePicker::Theme);
                self.ensure_invariants();
                true
            }
            SharedUiAction::Skills => {
                self.open_info_panel(InfoPanel::Skills);
                true
            }
        }
    }

    pub fn push_startup_logo(&mut self) {
        use rand::RngExt;
        let idx = rand::rng().random_range(0..STARTUP_LOGOS.len());
        let logo = STARTUP_LOGOS[idx];
        self.transcript.push_transcript_item(TranscriptEntry {
            id: 0,
            kind: TranscriptEntryKind::Logo(logo.to_string()),
            status: None,
        });
    }

    /// Clears the transcript (store) and resets the scroll position and token
    /// usage display. Cross-domain orchestration: transcript + scroll + status.
    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.scroll.scroll_offset = 0;
        self.scroll.following_tail = true;
        self.status.latest_input_tokens = None;
        self.status.latest_output_tokens = None;
        self.status.latest_total_tokens = None;
        self.scroll.entry_visual_info.clear();
        self.transcript.visual_info_dirty = true;
    }
}
