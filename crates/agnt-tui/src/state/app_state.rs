//! Application state types and the `AppState` reducer root.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use agnt_core::orchestrator::{OrchestratorEvent, UiRequest, UiStateEvent};
use agnt_core::protocol::contracts::SharedUiAction;
use agnt_core::transcript::entry::{BannerEntry, TranscriptEntry};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::compaction::CompactionState;
use super::hydration::hydrate_by_replay;
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
use crate::interaction::input::{TerminalEvent, map_terminal_event};
use crate::rendering::theme::{ThemeName, TuiTheme};
use crate::runtime::render::transcript::{PaintWindow, paint_transcript_rows};

const STARTUP_LOGOS: &[&str] = &[include_str!("../logos/00.txt")];

/// How long an armed Ctrl-C quit confirmation stays valid. Production keeps
/// the 5 s default; tests set [`AppState::quit_confirm_window`] directly.
pub(crate) const QUIT_CONFIRM_WINDOW: Duration = Duration::from_secs(5);

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
    pub description: Option<String>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallLine {
    pub id: u64,
    pub status: ToolCallStatus,
    pub key: agnt_core::protocol::events::ToolCallId,
    pub block_index: Option<usize>,
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
    /// When the Ctrl-C quit confirmation was armed, if it is armed.
    pub quit_confirm_since: Option<Instant>,
    /// How long an armed quit confirmation stays valid.
    pub quit_confirm_window: Duration,
    pub picker: PickerContainer,
    pub(crate) pending_switch_requests: VecDeque<SwitchRequest>,
    pub(crate) pending_launch: VecDeque<SharedUiAction>,
    pub(crate) pending_theme_persist: VecDeque<String>,
    pub(crate) pending_session_picker_refresh: bool,
    pub(crate) pending_agent_picker_refresh: bool,
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
    pub(crate) theme_name: ThemeName,
    pub event_tx: tokio::sync::mpsc::Sender<agnt_core::orchestrator::OrchestratorEvent>,
}

impl Default for AppState {
    fn default() -> Self {
        let (event_tx, _event_rx) =
            tokio::sync::mpsc::channel::<agnt_core::orchestrator::OrchestratorEvent>(256);
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
            pending_session_picker_refresh: false,
            pending_agent_picker_refresh: false,
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
            theme_name: ThemeName::default(),
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
        if self.take_next_session_picker_refresh_request() {
            events.push(OrchestratorEvent::UiRequest(
                UiRequest::RefreshSessionPicker,
            ));
        }
        if self.take_next_agent_picker_refresh_request() {
            events.push(OrchestratorEvent::UiRequest(UiRequest::RefreshAgentPicker));
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
        // The terminal layer reports keys, wheel notches, and resizes; the
        // reducer owns what they mean (the input lock, the open card, the
        // keymap).
        let Some(event) = map_terminal_event(event) else {
            return false;
        };
        crate::interaction::reducer::reduce(self, event, cancel_controller)
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
            } => hydrate_by_replay(self, messages, last_total_tokens),
            UiStateEvent::SetSessionPickerOptions(sessions) => {
                let tui_options: Vec<PickerOption> =
                    sessions.into_iter().map(PickerOption::from).collect();
                self.set_picker_options(ActivePicker::Session, tui_options)
            }
            UiStateEvent::SetAgentPickerOptions(summaries) => {
                // One scan feeds both surfaces: the picker rows and the Tab cycle.
                self.status.identity.agent_cycle_names =
                    summaries.iter().map(|s| s.name.clone()).collect();
                let catalog = agnt_core::protocol::picker::build_agent_picker_catalog(&summaries);
                let tui_options: Vec<PickerOption> =
                    catalog.into_iter().map(PickerOption::from).collect();
                self.set_picker_options(ActivePicker::Agent, tui_options);
                self.ensure_invariants();
            }
            UiStateEvent::EnqueueExternalPrompt { text } => {
                self.enqueue_prompt(text);
            }
            UiStateEvent::ExecuteSharedUiAction(action) => {
                self.execute_shared_ui_action(action);
            }
            UiStateEvent::PushStartupLogo => {
                self.push_startup_logo();
            }
        }
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
                self.picker
                    .position_selection_on(&self.status.identity.active_model_identity);
                self.ensure_invariants();
                true
            }
            SharedUiAction::Agents => {
                self.close_info_panel();
                self.picker.open(ActivePicker::Agent);
                self.pending_agent_picker_refresh = true;
                self.ensure_invariants();
                true
            }
            SharedUiAction::Sessions => {
                self.close_info_panel();
                self.picker.open(ActivePicker::Session);
                self.pending_session_picker_refresh = true;
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
                            ThemeName::CatppuccinFrappe => "Catppuccin Frappe".to_string(),
                            ThemeName::CatppuccinMacchiato => "Catppuccin Macchiato".to_string(),
                        };
                        PickerOption {
                            id: format!("{name:?}"),
                            display: display_name.clone(),
                            search_text: display_name.clone(),
                            section: None,
                            sort_key: Vec::new(),
                            payload: PickerPayload::Theme,
                        }
                    })
                    .collect();
                self.set_picker_options(ActivePicker::Theme, options);
                self.close_info_panel();
                self.picker.open(ActivePicker::Theme);
                // Highlight the active theme row.
                self.picker
                    .position_selection_on(&format!("{:?}", self.theme_name));
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
        let entry = TranscriptEntry::Banner(BannerEntry {
            text: logo.to_string(),
        });
        let evicted = self.transcript.push_entry(entry);
        self.shift_bookkeeping_after_eviction(evicted);
    }

    /// Clears the transcript (store) and resets the scroll position and token
    /// usage display. Cross-domain orchestration: transcript + scroll + status.
    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.scroll.scroll_offset = 0;
        self.scroll.following_tail = true;
        self.status.tokens.latest_input_tokens = None;
        self.status.tokens.latest_output_tokens = None;
        self.status.tokens.latest_total_tokens = None;
    }

    /// Builds the yank payload for the active visual-mode selection and clears
    /// it. The payload covers the selection's whole absolute visual-row range,
    /// independent of the viewport: the covering block range is rendered into
    /// an offscreen buffer at the transcript list width and scanned row by row,
    /// so the text keeps the displayed-row shape (lane prefixes, separator
    /// blanks, wrapped rows). Returns None when there is no selection or the
    /// range holds no text. The clipboard request stays with the reducer.
    pub fn yank_selection_payload(&mut self) -> Option<String> {
        let (start_row, end_row) = self.scroll.yank_selection()?;
        let payload = self.selection_text(start_row, end_row);
        if payload.is_empty() {
            None
        } else {
            Some(payload)
        }
    }

    /// Renders the transcript rows `start_row..=end_row` offscreen and returns
    /// them as newline-joined text.
    fn selection_text(&mut self, start_row: usize, end_row: usize) -> String {
        let width = self.scroll.transcript_area.width.saturating_sub(2) as usize;
        if width == 0 {
            return String::new();
        }
        if !self.transcript.height_index_valid_for(width) {
            self.transcript.rebuild_height_index(width);
        }
        let total_rows = self.transcript.total_visual_rows();
        if start_row >= total_rows {
            return String::new();
        }
        let end_row = end_row.min(total_rows.saturating_sub(1));
        let rows = end_row - start_row + 1;
        let area = Rect::new(
            0,
            0,
            width.min(u16::MAX as usize) as u16,
            rows.min(u16::MAX as usize) as u16,
        );
        let mut buffer = Buffer::empty(area);
        let window = PaintWindow {
            theme: &self.theme,
            width,
            first_row: start_row,
            height: rows,
            top: 0,
        };
        paint_transcript_rows(&self.transcript, &window, &mut buffer);
        let mut text_rows: Vec<String> = Vec::with_capacity(rows);
        for y in 0..area.height {
            let mut text = String::new();
            for x in 0..area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    text.push(cell.symbol().chars().next().unwrap_or(' '));
                }
            }
            text_rows.push(text.trim_end().to_string());
        }
        text_rows.join("\n")
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
